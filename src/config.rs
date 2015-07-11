//use std::collections::BTreeMap;
use std::path::{PathBuf};
use std::fs::{PathExt};

extern crate toml;

use util;
use mapping;

pub const KEY_SIZE: usize = 32;

pub struct SyncConfig {
    sync_dir: String,
    pub mapping: mapping::Mapping,
    pub encryption_key: Option<[u8; KEY_SIZE]>,
    pub syncdb_dir: Option<String>,
    pub native_paths: Vec<String>
}

#[cfg(feature = "release_paths")]
pub const BUILD_PREFIX:&'static str = "rel";
#[cfg(feature = "release_paths")]
pub const IS_REL:bool = true;

#[cfg(not(feature = "release_paths"))]
pub const BUILD_PREFIX:&'static str = "dbg";
#[cfg(not(feature = "release_paths"))]
pub const IS_REL:bool = false;

impl SyncConfig {
    pub fn sync_dir(&self) -> &str {
        &self.sync_dir
    }

    pub fn new(sync_dir: String,
        mapping: mapping::Mapping,
        ek:Option<[u8;KEY_SIZE]>,
        syncdb_dir:Option<String>,
        native_paths: Vec<String>) -> SyncConfig {
            let mut pb = PathBuf::from(&sync_dir);

            // let tests omit build prefix
            if !cfg!(test) {
                //error
                pb.push(BUILD_PREFIX);
            }

            let conf = SyncConfig {
                sync_dir: pb.to_str().unwrap().to_string(),
                mapping: mapping,
                encryption_key: ek,
                syncdb_dir: syncdb_dir,
                native_paths: native_paths
            };
            conf
    }
}

// TODO: this function should just return a Result instead of panicking
pub fn parse() -> SyncConfig {
    let mut file = "mapping".to_string();
    if !IS_REL {
        file.push_str(".");
        file.push_str(BUILD_PREFIX);
    };
    file.push_str(".toml");

    let toml = util::load_toml_file(&file);

    // verify config

    // read "General" section

    let general_section = {
        match toml.get("General") {
            None => panic!("No 'General' section in config file"),
            Some (thing) => {
                match thing.as_table() {
                    None => panic!("'General' must be a table, e.g.: [General]"),
                    Some (table) => {
                        table
                    }
                }
            }
        }
    };

    // get native paths
    let mut native_paths:Vec<String> = Vec::new();
    match general_section.get("NativePaths") {
        None => { panic!("No 'NativePaths' key found in config, cannot continue") },
        Some(paths) => {
            for p in paths.as_slice().unwrap() {
                native_paths.push(p.as_str().unwrap().to_string());
            }
        }
    }
    if native_paths.len() == 0 {
        panic!("No NativePaths are configured, cannot continue");
    }

    // host name mapping must exist
    let hn = util::get_hostname();
    // "." in the hostname, as is common on macs, will confuse toml.  look for the hostname
    // "." removed or replaced with "_"
    let hn = hn.replace(".", "_");
    let hn_key = format!("Machine_{}", hn);
    let (sync_dir, mapping) = {
        let mval = toml.get(&hn_key);
        //println!("{:?}: '{:?}' -> {:?}",toml,&hn_key,mval);
        let hn_config = match mval {
            None => {
                let mut helpmsg = String::new();
                if hn.find('.') != None {
                    helpmsg = format!("Try replacing '.' in your hostname with '_' in the configuration file: {}", hn_key.replace(".", "_"));
                }
                panic!("No hostname config found, cannot continue: {}\n{}", hn_key, helpmsg)
            },
            Some(c) => c
        };
        let hn_config = match hn_config.as_table() {
            None => { panic!("Hostname config must be a table") },
            Some(c) => c
        };
        let sync_dir = match hn_config.get(&"SyncDir".to_string()) {
            None => { panic!("No SyncDir specified for host") },
            Some (sd) => {
                let sd = sd.as_str().unwrap().to_string();
                let pp = PathBuf::from(&sd);
                if !pp.is_dir() {
                    panic!("Sync directory does not exist: {}", sd);
                }
                sd
            }
        };

        let mapping = match hn_config.get(&"Mapping".to_string()) {
            None => { panic!("No mapping for for host") },
            Some(m) => {
                match m.as_table() {
                    None => { panic!("Hostname mapping must be a table") },
                    Some(m) => {
                        let map_count = m.len();
                        if map_count == 0 {
                            panic!("No mapping entries found for host");
                        }
                        let mapping = mapping::Mapping::new(m);
                        match mapping {
                            Ok(m) => m,
                            Err(msg) => panic!(msg)
                        }
                    }
                }
            }
        };

        //println!("{:?}",mapping);

        (sync_dir, mapping)
    };

    // TODO: at some point I'm going to have to get this from somewhere!
    let mut ec: [u8;KEY_SIZE] = [0; KEY_SIZE];
    let mut next:u8 = 55;
    for i in 0..KEY_SIZE {
        ec[i] = next;
        next = next + 1;
    }

    let c = SyncConfig::new(
        sync_dir,
        mapping,
        Some(ec),
        None,
        native_paths
    );

    c
}
