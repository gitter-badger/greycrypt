#![feature(path_ext)]
use std::io;
use std::fs::{self, PathExt};
use std::path::{Path, PathBuf};
use std::collections::HashSet;

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

fn main() {
    println!("Welcome to the shit");

    let native_paths = vec![
        "C:\\Users\\John\\Documents\\GreyCryptTestSrc\\Nothere.txt",
        "C:\\Users\\John\\Documents\\GreyCryptTestSrc\\Another file.txt",
        "C:\\Users\\John\\Documents\\GreyCryptTestSrc",
        "C:\\Users\\John\\Documents\\GreyCryptTestFile.txt"];

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
