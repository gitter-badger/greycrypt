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

pub fn slurp_file(fname:&String) -> String {
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

pub fn load_toml_file(filename:&String) -> BTreeMap<String, toml::Value> {
    let toml = slurp_file(filename);
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

pub fn canon_path(p:String) -> String {
    p.replace("\\","/").to_string()
}
