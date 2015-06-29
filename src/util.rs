use std::io;
use std::fs::{self, PathExt};
use std::path::{Path, PathBuf};
use std::collections::BTreeMap;
use std::process::Command;

use std::fs::File;
use std::io::Read;

extern crate toml;

// From: https://doc.rust-lang.org/stable/std/fs/fn.read_dir.html
// walk_dir unstable, avoiding it for now
pub fn visit_dirs(dir: &Path, file_cb: &mut FnMut(&PathBuf)) -> io::Result<()> {
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

fn slurp_file<T,F>(fname:&str, slurper_fn: F ) -> T
    where F: Fn(File) -> Result<T,String> {
    let mut f = File::open(fname);
    match f {
        Err(e) => { panic!("Can't open file: {}: {}", fname, e) } ,
        Ok(f_h) => {
            let res = slurper_fn(f_h);
            match res {
                Err(e) => { panic!("Can't read file: {}: {}", fname, e) },
                Ok(v) => v
            }
        }
    }
}

pub fn slurp_bin_file(fname:&str) -> Vec<u8> {
    fn slurper (file:File) -> Result<Vec<u8>,String> {
        // avoid borrow error, though it generates a warning saying mut isn't needed
        let mut file = file;
        let mut data:Vec<u8> = Vec::new();
        let res = file.read_to_end(&mut data);
        match res {
            Err(e) => Err(format!("{:?}", e)),
            Ok(_) => Ok(data) // drop length and just return data
        }
    }

    let res = slurp_file(fname, slurper);
    res
}

pub fn slurp_text_file(fname:&String) -> String {
    fn slurper (file:File) -> Result<String,String> {
        // avoid borrow error, though it generates a warning saying mut isn't needed
        let mut file = file;
        let mut s = String::new();
        let res = file.read_to_string(&mut s);
        match res {
            Err(e) => Err(format!("{:?}", e)),
            Ok(_) => Ok(s) // drop length and just return data
        }
    }

    let res = slurp_file(fname, slurper);
    res
}

pub fn load_toml_file(filename:&String) -> BTreeMap<String, toml::Value> {
    let toml = slurp_text_file(filename);
    let res = toml::Parser::new(&toml).parse();
    let toml = match res {
        Some(value) => value,
        None => { panic!("Failed to parse toml file: {}", filename) }
    };

    toml
}

pub fn get_hostname() -> String {
    // no direct std function for this, as far as I can tell
    let output = Command::new("hostname")
                         .output()
                         .unwrap_or_else(|e| { panic!("failed to execute process: {}", e) });
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

pub fn canon_path(p:&str) -> String {
    let res = p.replace("\\","/").to_string();
    res
}

#[cfg(target_os = "windows")]
fn fixpath(p:&str) -> String {
    let res = p.replace("/","\\").to_string();
    res
}

#[cfg(not(target_os = "windows"))]
fn fixpath(p:&str) -> String {
    str.to_string()
}

pub fn decanon_path(p:&str) -> String {
    fixpath(p)
}
