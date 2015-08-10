//use std::fs::{PathExt,remove_file,remove_dir,read_dir};
use std::path::{PathBuf};
// use std::collections::HashSet;
// use std::collections::HashMap;
// use std::cmp::Ordering;

use config;
use syncfile;
use core;

#[allow(dead_code)]
pub fn show_syncfile_meta(state: &mut core::SyncState, filename:&str) {
    let syncpath = PathBuf::from(filename);
    let mdhash = match syncfile::SyncFile::get_metadata_hash(&state.conf,&syncpath) {
        Err(e) => panic!("{}", e),
        Ok(hash) => hash
    };

    let mut keys:Vec<String> = Vec::new();
    for (k,_) in &mdhash {
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
            } else {
                println!("binary file data omitted");
            }
        }
    }
}

#[allow(dead_code)]
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

#[cfg(not(test))]
fn collect_new_password() -> String {
    let new = config::pw_prompt(Some("Enter new password:"));
    let new2 = config::pw_prompt(Some("Confirm new password:"));
    
    if new != new2 {
        panic!("New passwords do not match");
    }
    new
}

#[cfg(test)]
fn collect_new_password() -> String {
    "swordfish".to_owned()
}

pub fn change_password(state: &mut core::SyncState) {
    let new_password = collect_new_password();
    
    let new_conf = state.conf.with_encryption_key(Some(config::get_encryption_key(&new_password))); 
    
    let syncfiles = core::find_all_syncfiles(state);
    
    let mut count = 0;
    for (_,files) in syncfiles.iter() {
        for f in files.iter() {
            // try to decode with new pw
            let syncfile = PathBuf::from(&f);
            let mut sf = match syncfile::SyncFile::from_syncfile(&new_conf,&syncfile) {
                Ok(_) => continue, // already updated
                Err(_) => {
                    // try old pw
                    match syncfile::SyncFile::from_syncfile(&state.conf,&syncfile) {
                        Err(e) => {
                            // fail, log and skip
                            warn!("Failed to decode syncfile with old & new password, skipping: {}; error: {}", f, e);
                            continue;
                        },
                        Ok(sf) => sf
                    }
                }
            };
            
            let mut data:Vec<u8> = Vec::new();

            match sf.decrypt_to_writer(&state.conf, &mut data) {
                Err(e) => panic!("Error decrypting file data for {}: {:?}", f, e),
                Ok(_) => {
                    // re-encrypt with new conf
                    match sf.save_with_data(&new_conf, Some(syncfile), data) {
                        Err(e) => panic!("Error encrypting file data for {}: {:?}", f, e),
                        Ok(_) => ()
                    }
                }
            }
            
            count = count + 1;
        }
    }
    state.conf = new_conf;
    info!("Password changed on {} sync files", count);   
}

#[cfg(test)]
mod tests {
    use core;
    use testlib::util::{basic_alice_bob_setup,verify_sync_state};
    
    #[test]
    fn change_password() {
        let (ref mut alice_mconf, _) = basic_alice_bob_setup("commands_change_password");
        
        core::do_sync(&mut alice_mconf.state);
        verify_sync_state(alice_mconf, 2, 2);
        
        let orig_ek = alice_mconf.state.conf.encryption_key.clone();
        
        super::change_password(&mut alice_mconf.state);
        
        assert!(alice_mconf.state.conf.encryption_key != None);
        assert!(alice_mconf.state.conf.encryption_key != orig_ek);
        
        verify_sync_state(alice_mconf, 2, 2);
    }
    
    #[test]
    #[should_panic(expected="are you using the correct password")]
    fn change_password_old_fails() {
        let (ref mut alice_mconf, _) = basic_alice_bob_setup("commands_change_password_old_fails");
        
        core::do_sync(&mut alice_mconf.state);
        verify_sync_state(alice_mconf, 2, 2);
        
        let orig_conf = alice_mconf.state.conf.clone();
        
        super::change_password(&mut alice_mconf.state);
        
        alice_mconf.state.conf = orig_conf;
        verify_sync_state(alice_mconf, 2, 2);
    }
}