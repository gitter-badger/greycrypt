//use std::collections::BTreeMap;
use std::path::{PathBuf};
use std::fs::{PathExt};

extern crate toml;

use util;
use mapping;

pub struct SyncConfig {
    pub sync_dir: String,
    pub mapping: mapping::Mapping,
    pub encryption_key: Option<[u8; 32]>,
    pub syncdb_dir: Option<String>,
    pub native_paths: Vec<String>
}

// TODO: this function should just return a Result instead of panicking
pub fn parse() -> SyncConfig {
    let toml = util::load_toml_file(&"mapping.toml".to_string());

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
    let hn_key = format!("Machine_{}", hn);
    let (sync_dir, mapping) = {
        let mval = toml.get(&hn_key);
        //println!("{:?}: '{:?}' -> {:?}",toml,&hn_key,mval);
        let hn_config = match mval {
            None => { panic!("No hostname config found, cannot continue: {}", hn_key) },
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
    let mut ec: [u8;32] = [0; 32];
    let mut next:u8 = 55;
    for i in 0..32 {
        ec[i] = next;
        next = next + 1;
    }

    let c = SyncConfig {
        sync_dir: sync_dir,
        mapping: mapping,
        encryption_key: Some(ec),
        syncdb_dir: None,
        native_paths: native_paths
    };

    c
}
