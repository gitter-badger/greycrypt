use std::fs::{PathExt};
use std::path::{Path,PathBuf};
use std::collections::HashSet;
use std::collections::HashMap;

use util;
use config;
use syncfile;
use syncdb;

#[derive(Debug,Clone)]
struct SyncData {
    syncid: String,
    syncfile: PathBuf,
    nativefile: Option<PathBuf>
}

#[derive(Debug,Clone)]
enum SyncAction {
    Nothing,
    CompareSyncState(SyncData),
    UpdateSyncfile(SyncData),
    CreateNewNativeFile(SyncData),
    CheckSyncRevguid(SyncData)
}

pub struct SyncState {
    pub conf: config::SyncConfig,
    pub syncdb: syncdb::SyncDb
}

fn compare_sync_state(state:&mut SyncState,sd:&SyncData) -> SyncAction {
    //println!("Comparing sync state on: {:?} and {:?}", sd.nativefile.file_name().unwrap(), sd.syncfile.file_name().unwrap());
    let nativefile = match sd.nativefile {
        None => panic!("Native file path must be set here"),
        Some (ref pathbuf) => pathbuf
    };
    let nativefile_str = nativefile.to_str().unwrap();

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
            panic!("Conflict on {:?}/{:?}; mtime_newer: {}, revguid_changed: {}", nativefile_str,
                sd.syncfile.file_name().unwrap(), native_newer, revguid_changed);
        },
        (true,false) => {
            println!("Would update native from syncfile, but I don't know how to do it: {}", nativefile_str);
            SyncAction::Nothing
        },
        (false,true) => {
            // Sync file needs update
            SyncAction::UpdateSyncfile(sd.clone())
        },
        (false,false) => {
            SyncAction::Nothing
        }
    }
}

fn update_sync_file(state:&mut SyncState,sd:&SyncData) -> SyncAction {
    let nativefile = match sd.nativefile {
        None => panic!("Native file path must be set here"),
        Some (ref pathbuf) => pathbuf
    };
    let nativefile_str = nativefile.to_str().unwrap();

    println!("Copying native data in {:?} to {:?}", nativefile_str, sd.syncfile.file_name().unwrap());

    let native_mtime = match util::get_file_mtime(&nativefile_str) {
        Err(e) => panic!("Error getting file mtime: {:?}", e),
        Ok(mtime) => mtime
    };

    match syncfile::SyncFile::create_syncfile(&state.conf,&nativefile) {
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

fn check_sync_revguid(state:&mut SyncState,sd:&SyncData) -> SyncAction {
    // found a sync file that has no corresponding native file.  So either:
    // 1) this is a new sync file, created elsewhere, that hasn't been synced here yet, or
    // 2) we deleted the file locally and this is a stale sync file that we should mark as deleted
    // we can differentiate the cases by looking at the syncdb state for the sid.  If the revguid of the
    // syncfile matches the db, we were the last ones to sync this file, so we can safely assume that it
    // was deleted locally (case 2).  otherwise, its a new sync file that we should copy to native dir.
    // NOTE: if we deleted the native file locally AND it was also changed on another machine, this
    // algorithm means we'll consider the sync file to be new, and restore the native file here.  This is probably
    // the safe option; if the user wants something deleted he should probably ensure that other systems
    // aren't changing it.
    if sd.nativefile != None {
        // wat
        panic!("Got check revguid action, but native file is set: {:?}", sd.nativefile);
    }

    //println!("Checking revguid on sid: {}",&sd.syncid);

    let sf = match syncfile::SyncFile::from_syncfile(&state.conf,&sd.syncfile) {
        Err(e) => panic!("Can't read syncfile: {:?}", e),
        Ok(sf) => sf
    };

    let sync_entry = state.syncdb.get(&sf);

    let new_sync_file = sync_entry.is_none() || sync_entry.unwrap().revguid != sf.revguid;

    if new_sync_file {
        // new sync file
        SyncAction::CreateNewNativeFile(sd.clone())
    } else {
        // local delete
        // TODO: this can fire erroneously if this machine isn't actually resyncing
        // the given native path; should handle that case (and maybe only unpack sync files
        // in the first place if the unpack directory is keyword-mapped)
        println!("Stale syncfile (revguid match), local file was deleted {:?}", &sd.syncid);

        // So, what we should do here is update the syncfile and set "Deleted", possibly with a
        // deletion time, in its metadata.
        // Other systems will need to handle that, both in CheckSyncRevguid and CompareSyncState.
        // If a file is deleted, they should remove/recycle the native file.  Unfortunately, with this method,
        // there is no way to know when everybody has processed the delete, so we have to leave the
        // sync file out there as a marker indefinitely (we will expunge the encrypted data, so at least
        // it small).  May want to implement some sort of time based garbage collection option.
        // Or a delete count in the file so the user can see how many systems processed the delete in some
        // kind of control panel.

        // for now, nothing.
        SyncAction::Nothing
    }
}

fn do_update_native_file(sf:&syncfile::SyncFile, state:&mut SyncState) {
    let res = sf.restore_native(&state.conf);
    let outfile = {
        match res {
            Err(e) => panic!("Error restoring native file: {:?}; {:?}", &sf.nativefile, e),
            Ok(outfile) => {
                // update syncdb
                let native_mtime = match util::get_file_mtime(&sf.nativefile) {
                    Err(e) => panic!("Error getting file mtime: {:?}", e),
                    Ok(mtime) => mtime
                };

                match state.syncdb.update(sf,native_mtime) {
                    Err(e) => panic!("Failed to update sync db: {:?}", e),
                    Ok(_) => ()
                }

                println!("Wrote new file: {:?}", &sf.nativefile);
            }
        }
    };
}

fn create_new_native_file(state:&mut SyncState,sd:&SyncData) -> SyncAction {
    let sf = match syncfile::SyncFile::from_syncfile(&state.conf,&sd.syncfile) {
        Err(e) => panic!("Can't read syncfile: {:?}", e),
        Ok(sf) => sf
    };

    // find target native path, if it already exists...well thats a problem
    let nativefile_path = PathBuf::from(&sf.nativefile);
    if nativefile_path.is_file() {
        panic!("Native path already exists for syncfile, refusing to overwrite: {}", &sf.nativefile);
    }
    do_update_native_file(&sf, state);
    SyncAction::Nothing
}

fn pass1_prep(state:&mut SyncState,sa:&SyncAction) -> SyncAction {
    match *sa {
        SyncAction::CompareSyncState(ref sd) => compare_sync_state(state,sd),
        SyncAction::CheckSyncRevguid(ref sd) => check_sync_revguid(state,sd),
        SyncAction::Nothing
        | SyncAction::UpdateSyncfile(_)
        | SyncAction::CreateNewNativeFile(_) => sa.clone()  // don't do this in pass1

    }
}
fn pass2_verify(state:&mut SyncState,sa:&SyncAction) -> SyncAction {
    match *sa {
        SyncAction::Nothing
        | SyncAction::UpdateSyncfile(_)
        | SyncAction::CreateNewNativeFile(_) => sa.clone(),
        SyncAction::CompareSyncState(_)
        | SyncAction::CheckSyncRevguid(_) => panic!("Cannot process action in this pass: {:?}", sa),
    }
}
fn pass3_commit(state:&mut SyncState,sa:&SyncAction) -> SyncAction {
    match *sa {
        SyncAction::Nothing => sa.clone(),
        SyncAction::UpdateSyncfile(ref sd) => update_sync_file(state,sd),
        SyncAction::CreateNewNativeFile(ref sd) => create_new_native_file(state,sd),
        SyncAction::CompareSyncState(_)
        | SyncAction::CheckSyncRevguid(_) => panic!("Cannot process action in this pass: {:?}", sa),
    }
}

pub fn do_sync(state:&mut SyncState) {
    let native_files = {
        // use hashset for path de-dup (TODO: but what about case differences?)
        let mut native_files = HashSet::new();
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

        native_files
    };

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

        let np = Some(PathBuf::from(&nf));
        let sd = SyncData { syncid: sid.to_string(), syncfile: syncfile.clone(), nativefile: np };
        if syncfile.is_file() {
            actions.insert(sid.to_string(), SyncAction::CompareSyncState(sd));
        } else {
            actions.insert(sid.to_string(), SyncAction::UpdateSyncfile(sd));
        }
    }

    // scan sync files
    let sync_files = {
        let sync_ext = "dat";

        let mut sync_files = HashSet::new();
        {
            let mut visitor = |pb: &PathBuf| {
                match pb.extension() {
                    None => {
                        return
                    }
                    Some(ext) => {
                        if ext.to_str().unwrap() != sync_ext {
                            return
                        }
                    }
                }

                // for now I'm going to ignore the google "conflict" files, which happens when two different
                // systems write different data to the same filename.  but I probably need a better strategy
                // for them.  TODO

                let pbs = pb.to_str().unwrap().to_string();
                if pb.file_stem().unwrap().to_str().unwrap().find(" ") != None {
                    //println!("ignore sf: {:?}", pbs);
                    return
                }
                //println!("sf: {:?}", pbs);
                sync_files.insert(pbs);
            };

            let d = &state.conf.sync_dir;
            let dp = Path::new(d);
            let res = util::visit_dirs(&dp, &mut visitor);
            match res {
                Ok(_) => (),
                Err(e) => panic!("failed to scan directory: {}: {}", d, e),
            }
        }
        sync_files
    };

    for sf in &sync_files {
        // sid is base filename without extension (assuming we are ignoring google " (1)" files)

        let syncfile = PathBuf::from(sf);
        let sid = syncfile.file_stem().unwrap().to_str().unwrap();

        // if we already have a compare action pending for the file, we don't need to crack it
        {
            let action = actions.get(sid);
            match action {
                None => (),
                Some(action) => {
                    match *action {
                        SyncAction::CompareSyncState(_) => continue, // skip
                        SyncAction::CheckSyncRevguid(_) => panic!("Check sync revguid shouldn't be here"),
                        SyncAction::CreateNewNativeFile(_) => panic!("Create new native file shouldn't be here"),
                        SyncAction::Nothing => (),
                        SyncAction::UpdateSyncfile(_) => ()
                    }
                }
            }
        }

        // no action yet, so we did not scan a native file that maps to the same sid.  have to check
        // revguids to see what to do.
        let sd = SyncData { syncid: sid.to_string(), syncfile: syncfile.clone(), nativefile: None };
        actions.insert(sid.to_string(), SyncAction::CheckSyncRevguid(sd));
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

#[cfg(test)]
mod tests {
    //use std::path::{Path, PathBuf};
    //use core;

    #[test]
    fn todo() {
    }
}