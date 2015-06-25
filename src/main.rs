#![feature(path_ext)]
use std::fs::{PathExt};
use std::path::{PathBuf};
use std::collections::HashSet;
use std::collections::BTreeMap;

extern crate toml;

mod util;

fn start_sync() {
    let native_paths:Vec<&String> = vec![];

    // use hashset for path de-dup
    let mut native_files = HashSet::new();

    // ownership of hashset must be transferred to closure for the enumeration, so use scope
    // block to release it
    {
        let mut visitor = |pb: &PathBuf| {
            native_files.insert(pb.to_str().unwrap().to_string());
        };

        for p in native_paths {
            let pp = PathBuf::from(p);
            if !pp.exists() {
                println!("WARN: path does not exist: {}", p);
            }
            if pp.is_file() {
                visitor(&pp);
            } else {
                let res = util::visit_dirs(pp.as_path(), &mut visitor);
                match res {
                    Ok(_) => (),
                    Err(e) => panic!("failed to scan directory: {}: {}", pp.to_str().unwrap(), e),
                }
            }
        };
    }

    for nf in native_files {
        println!("native file: {}", nf);
    }
}

struct SyncConfig {
    raw_toml: BTreeMap<String, toml::Value>,
    sync_dir: String
}

fn parse_config() -> SyncConfig {
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
                        } else {
                            println!("{} mapping entries found for this host", map_count);
                        }
                        m
                    }
                }
            }
        };

        //println!("{:?}",mapping);

        (sync_dir, mapping)
    };

    let c = SyncConfig {
        raw_toml: toml.clone(),
        sync_dir: sync_dir
    };

    println!("SyncDir: {:?}", c.sync_dir);
    c
}

fn main() {
    println!("Welcome to the shit");

    parse_config();
    start_sync();
}
