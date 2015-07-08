//use std::fs::{PathExt,remove_file,remove_dir,read_dir};
use std::path::{PathBuf};
// use std::collections::HashSet;
// use std::collections::HashMap;
// use std::cmp::Ordering;


use syncfile;
use core;

pub fn show_syncfile_meta(state: &mut core::SyncState, filename:&str) {
    let syncpath = PathBuf::from(filename);
    let mdhash = match syncfile::SyncFile::get_metadata_hash(&state.conf,&syncpath) {
        Err(e) => panic!("{}", e),
        Ok(hash) => hash
    };

    let mut keys:Vec<String> = Vec::new();
    for (k,v) in &mdhash {
        keys.push(k.to_string());
    }
    keys.sort();
    for k in keys {
        let v = mdhash.get(&k).unwrap();
        println!("{}: {}", k, v);
    }

    let mut sf = match syncfile::SyncFile::from_syncfile(&state.conf,&syncpath) {
        Err(e) => panic!("Failed to read syncfile: {:?}", e),
        Ok(sf) => sf
    };
    let entry = state.syncdb.get(&sf);
    match entry {
        None => println!("No sync db entry (file has not yet been processed on this machine)"),
        Some(entry) => {
            println!("syncdb entry:");
            println!("  for this file: {}", entry.revguid == sf.revguid);
            println!("  native_mtime: {}", entry.native_mtime);
            println!("  revguid: {}", entry.revguid);
        }
    }

    let mut data:Vec<u8> = Vec::new();

    match sf.decrypt_to_writer(&state.conf, &mut data) {
        Err(e) => panic!("Error {:?}", e),
        Ok(_) => {
            println!("decrypted size: {}", data.len());

            if !sf.is_binary {
                println!("text:");
                println!("{}", String::from_utf8(data).unwrap());
            }
        }
    }
}

pub fn show_conflicted_syncfile_meta(state: &mut core::SyncState) {
    let mut sync_files:Vec<String> = Vec::new();

    for (sid,files) in &state.sync_files_for_id {
        if state.is_conflicted(sid) {
            for f in files {
                sync_files.push(f.to_string());
            }
        }
    }

    for f in sync_files {
        println!("Showing conflicts (NOTE: dedup not run)");
        println!("Conflicted file: {}", f);
        show_syncfile_meta(state, &f);
        println!("");
    }
}
