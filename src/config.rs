use std::collections::BTreeMap;
use std::path::{PathBuf};
use std::fs::{PathExt};

extern crate toml;

use util;
use mapping;

pub struct SyncConfig {
    pub sync_dir: String,
    pub mapping: mapping::Mapping,
    pub encryption_key: Option<[u8; 32]>,
    pub syncdb_dir: Option<String>
}

pub fn parse() -> SyncConfig {
    let toml = util::load_toml_file(&"mapping.toml".to_string());

    // verify config

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

    let c = SyncConfig {
        sync_dir: sync_dir,
        mapping: mapping,
        encryption_key: None,
        syncdb_dir: None
    };

    c
}
