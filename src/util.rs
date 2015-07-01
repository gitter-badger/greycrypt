use std::io;
use std::fs::{self, PathExt};
use std::path::{Path, PathBuf};
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::process::Command;

#[cfg(not(target_os = "windows"))]
use std::os::unix::fs::MetadataExt;

use std::env;
use std::fs::File;
use std::io::Read;

use std::fs::{metadata};
#[cfg(target_os = "windows")]
use std::os::windows::fs::MetadataExt;

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
    let f = File::open(fname);
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

pub fn slurp_text_file(fname:&str) -> String {
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

pub fn load_toml_file(filename:&str) -> BTreeMap<String, toml::Value> {
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

// TODO: should just use serialization
pub fn string_lines_to_hashmap(lines:Vec<&str>) -> HashMap<String,String> {
    let mut hm:HashMap<String,String> = HashMap::new();
    for l in lines {
        // TODO: Actually, this should just split on the first ":"
        let parts:Vec<&str> = l.split(':').collect();
        let k = parts[0].trim();
        let v = parts[1].trim();
        hm.insert(k.to_lowercase(),v.to_string());
    }
    hm
}

#[cfg(target_os = "windows")]
fn fixpath(p:&str) -> String {
    let res = p.replace("/","\\").to_string();
    res
}

#[cfg(not(target_os = "windows"))]
fn fixpath(p:&str) -> String {
    p.to_string()
}

pub fn decanon_path(p:&str) -> String {
    fixpath(p)
}

#[cfg(target_os = "windows")]
pub fn get_file_mtime(path:&str) -> io::Result<u64> {
    let md = try!(metadata(&path));
    Ok(md.last_write_time())
}

#[cfg(not(target_os = "windows"))]
pub fn get_file_mtime(path:&str) -> io::Result<u64> {
    let md = try!(metadata(&path));
    let mtime = md.mtime();
    if mtime < 0 {
        panic!("Unexpected mtime, < 0: {:?}", mtime)
    }
    let umtime:u64 = mtime as u64;
    Ok(umtime)
}

#[cfg(target_os = "windows")]
pub fn get_appdata_dir() -> Option<String> {
    match env::var("APPDATA") {
        Err(_) => None,
        Ok(v) => Some(v.to_string())
    }
}

#[cfg(not(target_os = "windows"))]
pub fn get_appdata_dir() -> Option<String> {
    match env::var("HOME") {
        Err(_) => None,
        Ok(v) => {
            let mut pb = PathBuf::from(&v);
            pb.push(".greycrypt");
            Some(pb.to_str().unwrap().to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::path::{PathBuf};
    use util;

    #[test]
    fn file_slurp() {
        let wd = env::current_dir().unwrap();
        let mut testpath = PathBuf::from(&wd);
        testpath.push("testdata");
        testpath.push("test_native_file.txt");

        let path = testpath.to_str().unwrap();
        let srctext = util::slurp_text_file(path);
        let bintext = util::slurp_bin_file(path);
        let bin_to_text = String::from_utf8(bintext);
        assert!(bin_to_text.is_ok());
        assert_eq!(srctext, bin_to_text.unwrap());
    }
}
