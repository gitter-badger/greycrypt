#![feature(path_ext)]
use std::fs::{PathExt};
use std::path::{PathBuf};
use std::collections::HashSet;

mod util;
mod config;
mod mapping;

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

fn main() {
    let _ = config::parse();
    start_sync();
}
