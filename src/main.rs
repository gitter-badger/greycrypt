#![feature(path_ext)]
use std::io;
use std::fs::{self, PathExt};
use std::path::{Path, PathBuf};
use std::collections::HashSet;

extern crate toml;

use std::fs::File;
use std::io::Read;

// From: https://doc.rust-lang.org/stable/std/fs/fn.read_dir.html
// walk_dir unstable, avoiding it for now
fn visit_dirs(dir: &Path, file_cb: &mut FnMut(&PathBuf)) -> io::Result<()> {
    if dir.is_dir() {
        for entry in try!(fs::read_dir(dir)) {
            let entry = try!(entry);
            if entry.path().is_dir() {
                try!(visit_dirs(&entry.path(), file_cb));
            } else {
                file_cb(&entry.path());
            }
        }
    }
    Ok(())
}

fn start_sync() {
    let native_paths = vec![
        "C:\\Users\\John\\Documents\\GreyCryptTestSrc\\Nothere.txt",
        "C:\\Users\\John\\Documents\\GreyCryptTestSrc\\Another file.txt",
        "C:\\Users\\John\\Documents\\GreyCryptTestSrc"];

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
                let res = visit_dirs(pp.as_path(), &mut visitor);
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

fn slurp_file(fname:&String) -> String {
    // I can't believe that its really this terrible, but the example in the docs does not
    // compile: http://doc.rust-lang.org/std/fs/struct.File.html
    let mut f = File::open(fname);
    match f {
        Err(e) => { panic!("Can't open file: {}: {}", fname, e) } ,
        Ok(f_h) => {
            let mut f_h = f_h; // generates a warning, but without it, there is a borrow error in read_to_string below
            let mut s = String::new();
            let res = f_h.read_to_string(&mut s);
            match res {
                Err(e) => { panic!("Can't read file: {}: {}", fname, e) },
                Ok(_) => s
            }
        }
    }
}

fn parse_mappings() {
    let toml = slurp_file(&"mapping.toml".to_string());
    let res = toml::Parser::new(&toml).parse();
    match res {
        None => { panic!("Failed to parse mapping toml") }
        Some(value) => { println!("{:?}", value); }
    }
}

fn main() {
    println!("Welcome to the shit");

    parse_mappings();
    start_sync();
}
