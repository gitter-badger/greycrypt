// this is really spammy for me, will have to enable periodically
#![allow(dead_code)]
#![allow(unused_variables)]

#![feature(path_ext)]
use std::fs::{PathExt};
use std::path::{PathBuf};
use std::collections::HashSet;
use std::collections::HashMap;

mod util;
mod config;
mod mapping;
mod syncfile;
mod crypto_util;
mod syncdb;

struct SyncData {
    syncid: String,
    syncfile: PathBuf,
    nativefile: PathBuf
}

enum SyncAction {
    Nothing,
    CompareSyncState(SyncData),
    UpdateSyncfile(SyncData)
}

// These functions exist because I don't know how to create a new value type from a reference
// TODO: really need to learn how to copy from reference, but its an obscure topic for googling.
// Its probably in the guide somewhere.
// *X has something do with it, but can't use that here because its a move out of a borrowed context
fn clone_syncdata(sd:&SyncData) -> SyncData {
    SyncData {
        syncid: sd.syncid.clone(),
        syncfile: sd.syncfile.clone(),
        nativefile: sd.nativefile.clone()
    }
}

fn clone_action(a:&SyncAction) -> SyncAction {
    match *a {
        SyncAction::Nothing => SyncAction::Nothing,
        SyncAction::CompareSyncState(ref sd) => SyncAction::CompareSyncState(clone_syncdata(sd)),
        SyncAction::UpdateSyncfile(ref sd) => SyncAction::UpdateSyncfile(clone_syncdata(sd)),
    }
}

struct SyncState {
    conf: config::SyncConfig,
    syncdb: syncdb::SyncDb
}

fn compare_sync_state(state:&mut SyncState,sd:&SyncData) -> SyncAction {
    //println!("Comparing sync state on: {:?} and {:?}", sd.nativefile.file_name().unwrap(), sd.syncfile.file_name().unwrap());
    let nativefile_str = sd.nativefile.to_str().unwrap();

    let native_mtime = match util::get_file_mtime(&nativefile_str) {
        Err(e) => panic!("Error getting file mtime: {:?}", e),
        Ok(mtime) => mtime
    };
    let sf = match syncfile::SyncFile::from_syncfile(&state.conf,&sd.syncfile) {
        Err(e) => panic!("Can't read syncfile: {:?}", e),
        Ok(sf) => sf
    };
    let sync_entry = match state.syncdb.get(&sf) {
        None => panic!("File should have an entry in syncdb, but does not: {:?}", &sd.syncfile),
        Some(entry) => entry
    };

    let revguid_changed = sf.revguid != sync_entry.revguid;
    let native_newer = native_mtime > sync_entry.native_mtime;

    match (revguid_changed,native_newer) {
        (true,true) => {
            // conflict! for now, panic
            panic!("Conflict on {:?}/{:?}; mtime_newer: {}, revguid_changed: {}", sd.nativefile.file_name().unwrap(),
                sd.syncfile.file_name().unwrap(), native_newer, revguid_changed);
        },
        (true,false) => {
            println!("Would update native from syncfile, but I don't know how to do it: {}", nativefile_str);
            SyncAction::Nothing
        },
        (false,true) => {
            // Sync file needs update
            SyncAction::UpdateSyncfile(clone_syncdata(sd))
        },
        (false,false) => {
            SyncAction::Nothing
        }
    }
}

fn update_sync_file(state:&mut SyncState,sd:&SyncData) -> SyncAction {
    println!("Copying native data in {:?} to {:?}", sd.nativefile.file_name().unwrap(), sd.syncfile.file_name().unwrap());

    let nativefile_str = sd.nativefile.to_str().unwrap();

    let native_mtime = match util::get_file_mtime(&nativefile_str) {
        Err(e) => panic!("Error getting file mtime: {:?}", e),
        Ok(mtime) => mtime
    };

    match syncfile::SyncFile::create_syncfile(&state.conf,&sd.nativefile) {
        Err(e) => panic!("Error creating sync file: {:?}", e),
        Ok((ref sfpath,ref sf)) => {
            // update sync db
            match state.syncdb.update(sf,native_mtime) {
                Err(e) => panic!("Failed to update sync db: {:?}", e),
                Ok(_) => ()
            }
        }
    };

    SyncAction::Nothing
}

fn pass1_prep(state:&mut SyncState,sa:&SyncAction) -> SyncAction {
    match *sa {
        SyncAction::Nothing => clone_action(sa),
        SyncAction::CompareSyncState(ref sd) => compare_sync_state(state,sd),
        SyncAction::UpdateSyncfile(_) => clone_action(sa), // don't do this in pass1
    }
}
fn pass2_verify(state:&mut SyncState,sa:&SyncAction) -> SyncAction {
    match *sa {
        SyncAction::Nothing => clone_action(sa),
        SyncAction::CompareSyncState(ref sd) => panic!("Cannot compare sync state in this pass"),
        SyncAction::UpdateSyncfile(_) => clone_action(sa),
    }
}
fn pass3_commit(state:&mut SyncState,sa:&SyncAction) -> SyncAction {
    match *sa {
        SyncAction::Nothing => clone_action(sa),
        SyncAction::CompareSyncState(ref sd) => panic!("Cannot compare sync state here"),
        SyncAction::UpdateSyncfile(ref sd) => update_sync_file(state,sd),
    }
}

fn do_sync(state:&mut SyncState) {
    // use hashset for path de-dup
    let mut native_files = HashSet::new();

    // ownership of hashset must be transferred to closure for the enumeration, so use scope
    // block to release it
    {
        let mut visitor = |pb: &PathBuf| {
            native_files.insert(pb.to_str().unwrap().to_string());
        };

        let native_paths = &state.conf.native_paths;
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

    //let mut actions:Vec<SyncAction> = Vec::new();
    let mut actions:HashMap<String,SyncAction> = HashMap::new();

    // scan native files
    for nf in &native_files {
        //println!("native file: {}", nf);
        let (sid,syncfile) = match syncfile::SyncFile::get_sync_id_and_path(&state.conf,&nf) {
            Err(e) => {
                println!("Ignoring native file: {}: {}", &nf, &e);
                continue
            },
            Ok(pair) => pair
        };

        if actions.contains_key(&sid) {
            panic!("Unexpected error: action already present for file: {}", nf)
        }

        let np = PathBuf::from(&nf);
        let sd = SyncData { syncid: sid.to_string(), syncfile: syncfile.clone(), nativefile: np };
        if syncfile.is_file() {
            actions.insert(sid.to_string(), SyncAction::CompareSyncState(sd));
        } else {
            actions.insert(sid.to_string(), SyncAction::UpdateSyncfile(sd));
        }
    }

    // TODO: use map() once I figure how to match on the struct references
    //let actions = actions.iter().map(|a| pass1_action_handler(a) );

    fn process_actions<F>(state:&mut SyncState, actions:&HashMap<String,SyncAction>, act_fn: &mut F ) -> HashMap<String,SyncAction>
        where F: FnMut(&mut SyncState, &SyncAction) -> SyncAction {
            let mut new_actions:HashMap<String,SyncAction> = HashMap::new();
            for (sid,action) in actions {
                new_actions.insert(sid.to_string(), act_fn(state,action));
            }
            new_actions
    }

    let actions = process_actions(state, &actions, &mut pass1_prep);
    let actions = process_actions(state, &actions, &mut pass2_verify);
    let actions = process_actions(state, &actions, &mut pass3_commit);
}

fn main() {
    let conf = config::parse();
    let syncdb = match syncdb::SyncDb::new(&conf) {
        Err(e) => panic!("Failed to create syncdb: {:?}", e),
        Ok(sdb) => sdb
    };

    let mut state = SyncState {
        syncdb: syncdb,
        conf: conf
    };
    do_sync(&mut state);
}
