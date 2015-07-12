use std::collections::BTreeMap;
use std::collections::HashSet;
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
    
    let (sync_dir, mapping) = {
        let mval = match toml.get("Mapping") {
            None => panic!("Unable to find [Mapping] in toml file"),
            Some (mval) => mval
        };
        let mval = match mval.as_table() {
            None => panic!("Mapping object must be table, like this: [Mapping]"),
            Some (mval) => mval
        };
        
        let mut map_nicknames:HashSet<String> = HashSet::new();
        for (map_nick,hn_list) in mval {
            match hn_list.as_slice() {
                None => panic!("The value for map nick {} must be a list, e.g [\"myhostname\"]", map_nick),
                Some(hn_list) => {
                    for lhn in hn_list {
                        match lhn.as_str() {
                            None => panic!("The values in map nick list {} must be strings, e.g. e.g [\"myhostname\"]", map_nick),
                            Some (lhn) => {
                                if lhn == hn {
                                    map_nicknames.insert(map_nick.to_string());
                                }                            
                            }
                        }
                    }
                }
            } 
        }
        
        // host must be mapped to 1 nick
        if map_nicknames.len() == 0 {
            panic!("Unable to map hostname: try adding a line to [Mapping] like this: mynick = [\"{}\"]", hn); 
        }
        if map_nicknames.len() > 1 {
            panic!("Too many mappings for hostname found, make sure [Mapping] contains only one relationship for host {}", hn);
        }

        let map_nick = map_nicknames.iter().nth(0).unwrap();        
        
        // now try to find a HostDef.<hostname> object
        let hn_map_key = format!("HostDef-{}", map_nick);
        let hn_config = match toml.get(&hn_map_key) {
            None => panic!("No host definition found, try adding [{}]", hn_map_key),
            Some(hn_config) => {
                match hn_config.as_table() {
                    None => panic!("Hostname config must be a table, e.g. [{}]", hn_map_key),
                    Some(c) => c
                }
            } 
        };
        
        // find key in hn_config and return its value; panics if not found.
        let mut hn_config = hn_config.clone();
        let get_and_remove = |hn_config:&mut BTreeMap<String, toml::Value>,key:&str| {
            let sd = match hn_config.get(key) {
                None => { panic!("No {} specified for host in {}", key, &hn_map_key) },
                Some (sd) => {
                    sd.clone()
                }
            };
            hn_config.remove(key);
            sd
        };
        
        let sync_dir = match get_and_remove(&mut hn_config,"SyncDir").as_str() {
            None => panic!("Value for SyncDir must be a string"),
            Some (sd) => {
                let pp = PathBuf::from(&sd);
                if !pp.is_dir() {
                    panic!("Sync directory does not exist: {}", sd);
                }
                sd.to_string()                
            }
        };
        
        // all the other key/value pairs are kw->dir mappings
        let map_count = hn_config.len();
        if map_count == 0 {
            let mut helpstr = String::new();
            helpstr.push_str("No mapping entries found for host\n");
            helpstr.push_str(&format!("Add lines to [{}] with this format: keyword = \"/path/to/local/dir\"\n", hn_map_key));
            helpstr.push_str("Note: on windows, double backslash is required: home = \"C:\\\\Users\\\\Fred\"");
            panic!(helpstr);
        }

        let mapping = match mapping::Mapping::new(&hn_config) {
            Ok(m) => m,
            Err(msg) => panic!(msg)
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
