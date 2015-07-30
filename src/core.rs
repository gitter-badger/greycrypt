use std::fs::{PathExt,remove_file,remove_dir,read_dir};
//use std::io::{BufRead};
use std::path::{Path,PathBuf};
use std::collections::HashSet;
use std::collections::HashMap;
use std::cmp::Ordering;

use util;
use config;
use syncfile;
use syncdb;
use trash;
use logging;

extern crate glob;

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
    UpdateNativeFile(SyncData),
    ProcessNativeDelete(SyncData),
    ProcessSyncfileDelete(SyncData),
    CreateNewNativeFile(SyncData),
    CheckSyncRevguid(SyncData),
    CheckFilesEqualElseConflict(SyncData)
}

pub struct SyncFileCache {
    map: HashMap<String, syncfile::SyncFile>
}

impl SyncFileCache {
    pub fn new() -> Self {
        SyncFileCache {
            map: HashMap::new()
        }
    }

    pub fn get(&mut self, conf: &config::SyncConfig, syncfile: &PathBuf) -> &syncfile::SyncFile {
        let pbs = match syncfile.to_str() {
            None => panic!("Failed to make string from pathbuf: {:?}", syncfile),
            Some (pbs) => pbs.to_owned()
        };

        if !self.map.contains_key(&pbs) {
            let mut sf = match syncfile::SyncFile::from_syncfile(&conf,syncfile) {
                Err(e) => panic!("Can't read syncfile {:?}: {:?}", syncfile, e),
                Ok(sf) => sf
            };
            // close the sync file, which means that entries from the cache can't be used to read the actual data
            sf.close();

            self.map.insert(pbs.clone(), sf);
        }
        self.map.get(&pbs).unwrap()
    }

    pub fn flush(&mut self) {
        self.map.clear();
    }
}

pub struct SyncState {
    pub conf: config::SyncConfig,
    pub syncdb: syncdb::SyncDb,
    pub sync_files_for_id: HashMap<String,Vec<String>>,
    pub sync_file_cache: SyncFileCache,
    pub log_util: logging::LoggerUtil
}

impl SyncState {
    pub fn new(conf:config::SyncConfig, syncdb: syncdb::SyncDb, log_util: logging::LoggerUtil) -> Self {
        SyncState {
            syncdb: syncdb,
            conf: conf,
            sync_files_for_id: HashMap::new(),
            sync_file_cache: SyncFileCache::new(),
            log_util: log_util
        }
    }  
    
    pub fn is_conflicted(&self,sid:&str) -> bool {
        match self.sync_files_for_id.get(sid) {
            None => false,
            Some(files) => files.len() > 1
        }
    }
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
    let sf = state.sync_file_cache.get(&state.conf,&sd.syncfile);

    let sync_entry = match state.syncdb.get(&sf) {
        None => {
            if sf.is_deleted {
                return SyncAction::ProcessSyncfileDelete(sd.clone());
            } else {
                return SyncAction::CheckFilesEqualElseConflict(sd.clone());
            }
        }
        Some(entry) => entry
    };

    if sf.is_deleted {
        let revguid_changed = sf.revguid != sync_entry.revguid;

        if revguid_changed {
            //  if revguid doesn't match, either
            //   1) the checksum of our native data matches the checksum of the deleted data, in which case
            //     we can delete thise file
            //   2) the checksum doesn't match, in which case, the native file was updated and we
            //     have a conflict because the incoming SF wants a delete.

            return SyncAction::ProcessSyncfileDelete(sd.clone());
        } else {
            //  if the revguid matches, but native mtime is newer than the syncdb mtime
            // then native has been recreated with new data: update sync file.

            let native_newer = native_mtime > sync_entry.native_mtime; // TODO, always gonna be true if I set mtime to 0 on delete!
            if native_newer {
                return SyncAction::UpdateSyncfile(sd.clone());
            } else {
                return SyncAction::Nothing;
            }
        }
    } else {
        let revguid_changed = sf.revguid != sync_entry.revguid;
        let native_newer = native_mtime > sync_entry.native_mtime;

        match (revguid_changed,native_newer) {
            (true,true) => {
                // conflict! for now, panic
                panic!("Conflict on {:?}/{:?}; mtime_newer: {}, revguid_changed: {}", nativefile_str,
                    sd.syncfile.file_name().unwrap(), native_newer, revguid_changed);
            },
            (true,false) => {
                SyncAction::UpdateNativeFile(sd.clone())
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
}

fn update_native_file(state:&mut SyncState,sd:&SyncData) -> SyncAction {
    let native_fname = match sd.nativefile {
        None => panic!("Native file required"),
        Some(ref fname) => fname
    };

    let (equal,sf) = match check_file_data_equal(state, &sd.syncfile, &native_fname) {
        Err(e) => panic!("Error checking file data: {}", e),
        Ok(stuff) => stuff
    };

    let outfile = {
        if equal {
            info!("Native file matches local, updating syncdb: {:?}", sf.nativefile);
            native_fname.to_str().unwrap().to_owned()
        } else {
            // need to use a new SF here to unpack data,
            // because the one used for the equal check has
            // already been "expended"
            let mut sf = match syncfile::SyncFile::from_syncfile(&state.conf,&sd.syncfile) {
                Err(e) => panic!("Can't read syncfile {:?}: {:?}", &sd.syncfile, e),
                Ok(sf) => sf
            };
            info!("Updating native file: {:?}", sf.nativefile);
            let res = sf.restore_native(&state.conf);
            let outfile = {
                match res {
                    Err(e) => panic!("Error updating native file {:?}", e),
                    Ok(outfile) => outfile
                }
            };
            outfile
        }
    };

    let native_mtime = match util::get_file_mtime(&outfile) {
        Err(e) => panic!("Error getting file mtime: {:?}", e),
        Ok(mtime) => mtime
    };
    match state.syncdb.update(&sf,native_mtime) {
        Err(e) => panic!("Failed to update sync db: {:?}", e),
        Ok(_) => ()
    };
    SyncAction::Nothing
}

fn update_sync_file(state:&mut SyncState,sd:&SyncData) -> SyncAction {
    let nativefile = match sd.nativefile {
        None => panic!("Native file path must be set here"),
        Some (ref pathbuf) => pathbuf
    };
    let nativefile_str = nativefile.to_str().unwrap();

    info!("Copying native data in {:?} to {:?}", nativefile_str, sd.syncfile.file_name().unwrap());

    let native_mtime = match util::get_file_mtime(&nativefile_str) {
        Err(e) => panic!("Error getting file mtime: {:?}", e),
        Ok(mtime) => mtime
    };

    // always use the path from the sync data struct, since it may have been remapped
    match syncfile::SyncFile::create_syncfile(&state.conf,&nativefile, Some(sd.syncfile.clone())) {
        Err(e) => panic!("Error creating sync file: {:?}", e),
        Ok((_,ref sf)) => {
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

    let sf = state.sync_file_cache.get(&state.conf,&sd.syncfile);

    let sync_entry = state.syncdb.get(&sf);

    if sf.is_deleted {
        if !sync_entry.is_none() && sync_entry.unwrap().revguid != sf.revguid {
            // the native file is already gone, but let the action handle this case (to update syncdb, etc)
            return SyncAction::ProcessNativeDelete(sd.clone());
            //println!("Ignoring deleted syncfile with no corresponding native file: {:?}", &sd.syncid);
        }
        // nothing to do
        return SyncAction::Nothing;
    }

    let new_sync_file = sync_entry.is_none() || sync_entry.unwrap().revguid != sf.revguid;

    if new_sync_file {
        // new sync file
        SyncAction::CreateNewNativeFile(sd.clone())
    } else {
        // local delete
        info!("Stale syncfile (revguid match), local file was deleted {:?}", &sd.syncid);

        SyncAction::ProcessNativeDelete(sd.clone())
    }
}

fn check_file_data_equal(state:&mut SyncState,syncfile:&PathBuf,nativefile:&PathBuf) -> Result<(bool,syncfile::SyncFile),String> {
    let mut sf_data:Vec<u8> = Vec::new();
    let syncpath = syncfile.to_str().unwrap().to_owned();
    let sf = load_syncfile_or_panic(state,&syncpath,&mut sf_data);
    // if file is text, syncfile decryption will have decanoned the lines, so we can compare them
    // directly with native line format.  so use binary read for both text and binary files.
    let native_bytes = util::slurp_bin_file(&nativefile.to_str().unwrap());

    let native_bytes = &native_bytes[0 .. native_bytes.len()];
    let sf_bytes = &sf_data[0 .. sf_data.len()];

    if native_bytes == sf_bytes {
        Ok((true,sf))
    } else {
        Ok((false,sf))
    }
}

fn check_files_equal_else_conflict(state:&mut SyncState,sd:&SyncData) -> SyncAction {
   // This will happen if we start syncing on a new machine, and it already has a copy
   // of the native file.  If the file contents are an exact match, we can ignore this
   // and just update the local syncdb.  Otherwise, its a conflict.

   let native_fname = match sd.nativefile {
       None => panic!("Native file required"),
       Some(ref fname) => fname
   };

   let (equal,sf) = match check_file_data_equal(state, &sd.syncfile, &native_fname) {
       Err(e) => panic!("Error checking file data: {:?}: {}", native_fname, e),
       Ok(stuff) => stuff
   };

   if equal {
       let native_fname = native_fname.to_str().unwrap();
       // update syncdb
       let native_mtime = match util::get_file_mtime(&native_fname) {
           Err(e) => panic!("Error getting file mtime: {:?}: {}", native_fname, e),
           Ok(mtime) => mtime
       };

       match state.syncdb.update(&sf,native_mtime) {
           Err(e) => panic!("Failed to update sync db: {:?}: {}", native_fname, e),
           Ok(_) => ()
       }
   } else {
       warn!("Conflict detected on {:?}, local data differs from remote.  Try renaming local file and resyncing to restore remote data.", native_fname);
   }

   SyncAction::Nothing
}


fn do_update_native_file(sf:&mut syncfile::SyncFile, state:&mut SyncState) {
    let res = sf.restore_native(&state.conf);

    match res {
        Err(e) => panic!("Error restoring native file: {:?}; {:?}", &sf.nativefile, e),
        Ok(_) => {
            // update syncdb
            let native_mtime = match util::get_file_mtime(&sf.nativefile) {
                Err(e) => panic!("Error getting file mtime: {:?}; {:?}", &sf.nativefile, e),
                Ok(mtime) => mtime
            };

            match state.syncdb.update(sf,native_mtime) {
                Err(e) => panic!("Failed to update sync db: {:?}; {:?}", &sf.nativefile, e),
                Ok(_) => ()
            }

            info!("Wrote new file: {:?}", &sf.nativefile);
        }
    };
}

fn create_new_native_file(state:&mut SyncState,sd:&SyncData) -> SyncAction {
    let mut sf = match syncfile::SyncFile::from_syncfile(&state.conf,&sd.syncfile) {
        Err(e) => panic!("Can't read syncfile {:?}: {:?}", &sd.syncfile, e),
        Ok(sf) => sf
    };

    // find target native path, if it already exists...well thats a problem
    let nativefile_path = PathBuf::from(&sf.nativefile);
    if nativefile_path.is_file() {
        panic!("Native path already exists for syncfile, refusing to overwrite: {}", &sf.nativefile);
    }
    do_update_native_file(&mut sf, state);
    SyncAction::Nothing
}

fn handle_delete(state:&mut SyncState, sf:&mut syncfile::SyncFile, syncpath: &PathBuf, mark_sf_as_deleted:bool) {
    let nativefile_path = PathBuf::from(&sf.nativefile);
    if nativefile_path.is_file() {
        info!("Sending deleted local file to Trash: {}", &sf.nativefile);
        match trash::send_to_trash(&sf.nativefile) {
            Err(e) => panic!("Failed to trash file: {:?}", e),
            Ok(_) => ()
        }
    }
    if mark_sf_as_deleted {
        match sf.mark_deleted_and_save(&state.conf,Some(syncpath.clone())) {
            Err(e) => panic!("Failed to write syncfile: {:?}", e),
            Ok(_) => ()
        };
    }

    // update syncdb
    let native_mtime = 0;

    match state.syncdb.update(&sf,native_mtime) {
        Err(e) => panic!("Failed to update sync db: {:?}; {:?}", &sf.nativefile, e),
        Ok(_) => ()
    }
}

fn process_native_delete(state:&mut SyncState,sd:&SyncData) -> SyncAction {
    // So, what we should do here is update the syncfile and set "Deleted", possibly with a
    // deletion time, in its metadata.
    // Other systems will need to handle that, both in CheckSyncRevguid and CompareSyncState.
    // If a file is deleted, they should remove/recycle the native file.  Unfortunately, with this method,
    // there is no way to know when everybody has processed the delete, so we have to leave the
    // sync file out there as a marker indefinitely (we will expunge the encrypted data, so at least
    // it small).  May want to implement some sort of time based garbage collection option.
    // Or a delete count in the file so the user can see how many systems processed the delete in some
    // kind of control panel.
    let mut sf = match syncfile::SyncFile::from_syncfile(&state.conf,&sd.syncfile) {
        Err(e) => panic!("Can't read syncfile {:?}: {:?}", &sd.syncfile, e),
        Ok(sf) => sf
    };
    handle_delete(state, &mut sf, &sd.syncfile, true);

    SyncAction::Nothing
}

fn process_syncfile_delete(state:&mut SyncState,sd:&SyncData) -> SyncAction {
    let mut sf = match syncfile::SyncFile::from_syncfile(&state.conf,&sd.syncfile) {
        Err(e) => panic!("Can't read syncfile {:?}: {:?}", &sd.syncfile, e),
        Ok(sf) => sf
    };
    // sanity
    if !sf.is_deleted {
        panic!("Attempting to delete native file using a non-deleted syncfile!")
    }

    // so, here's a dilemma, if the sync file is marked as deleted and we have no syncdb entry for this
    // guy, should we delete the native file?
    // ideally we'd have a checksum on the data of the native file, so we could compare that
    // with the checksum at time of deletion, and only delete if it matches.
    // for now I'm just gonna panic if I detect this case.
    let revguid = {
        let sync_entry = match state.syncdb.get(&sf) {
            None => {
                // here we would need to check to see if the native file has the same checksum
                // as deleted - actually, maybe we want to check that no matter what
                panic!("Refusing to delete native file; no syncdb entry exists: {:?}",sf.nativefile);
            }
            Some(entry) => entry
        };
        sync_entry.revguid
    };

    // if the revguids are different we can delete the native file
    // TODO: also check checksum when we have that; if that fails then this is a conflict
    if sf.revguid != revguid {
        // don't mark it as deleted because its already marked as such (and doing so
        // would generate another revguid, causing churn on remote systems)
        handle_delete(state, &mut sf, &sd.syncfile, false);
        SyncAction::Nothing
    } else {
        // we should have already handled this...but log if the native file exists (bug)
        let pb = PathBuf::from(&sf.nativefile);
        if pb.is_file() {
            error!("Left behind a file that should have been deleted: {:?}", sf.nativefile);
        }
        SyncAction::Nothing
    }
}

fn pass1_prep(state:&mut SyncState,sa:&SyncAction) -> SyncAction {
    match *sa {
        SyncAction::CompareSyncState(ref sd) => compare_sync_state(state,sd),
        SyncAction::CheckSyncRevguid(ref sd) => check_sync_revguid(state,sd),
        SyncAction::Nothing
        | SyncAction::ProcessNativeDelete(_)
        | SyncAction::ProcessSyncfileDelete(_)
        | SyncAction::UpdateSyncfile(_)
        | SyncAction::UpdateNativeFile(_)
        | SyncAction::CheckFilesEqualElseConflict(_)
        | SyncAction::CreateNewNativeFile(_) => sa.clone()  // don't do this in pass1

    }
}
fn pass2_verify(state:&mut SyncState,sa:&SyncAction) -> SyncAction {
    // this function doesn't do anything with state ATM, but it is required for the pass interface.
    // silence warning
    let _ = state;
    match *sa {
        SyncAction::Nothing
        | SyncAction::ProcessNativeDelete(_)
        | SyncAction::ProcessSyncfileDelete(_)
        | SyncAction::UpdateSyncfile(_)
        | SyncAction::UpdateNativeFile(_)
        | SyncAction::CheckFilesEqualElseConflict(_)
        | SyncAction::CreateNewNativeFile(_) => sa.clone(),
        SyncAction::CompareSyncState(_)
        | SyncAction::CheckSyncRevguid(_) => panic!("Cannot process action in this pass: {:?}", sa),
    }
}
fn pass3_commit(state:&mut SyncState,sa:&SyncAction) -> SyncAction {
    match *sa {
        SyncAction::Nothing => sa.clone(),
        SyncAction::CheckFilesEqualElseConflict(ref sd) => check_files_equal_else_conflict(state,sd),
        SyncAction::UpdateNativeFile(ref sd) => update_native_file(state,sd),
        SyncAction::UpdateSyncfile(ref sd) => update_sync_file(state,sd),
        SyncAction::CreateNewNativeFile(ref sd) => create_new_native_file(state,sd),
        SyncAction::ProcessNativeDelete(ref sd) => process_native_delete(state,sd),
        SyncAction::ProcessSyncfileDelete(ref sd) => process_syncfile_delete(state,sd),
        SyncAction::CompareSyncState(_)
        | SyncAction::CheckSyncRevguid(_) => panic!("Cannot process action in this pass: {:?}", sa),
    }
}

pub fn find_all_syncfiles(state:&SyncState) -> HashMap<String,Vec<String>> {
    let sync_ext = "dat";

    let mut files_for_id:HashMap<String,Vec<String>> = HashMap::new();
    {
        let mut visitor = |pb: &PathBuf| {
            match pb.extension() {
                None => return,
                Some(ext) => {
                    if ext.to_str().unwrap() != sync_ext {
                        return
                    }
                }
            }

            // have to read at least the first line to get the syncid.  can't trust the
            // filename because it could have been renamed.
            let file_syncid = match syncfile::SyncFile::get_syncid_from_file(&pb) {
                Err(e) => panic!("Error {:?}", e),
                Ok(id) => id
            };

            let pbs = pb.to_str().unwrap().to_owned();
            if files_for_id.contains_key(&file_syncid) {
                files_for_id.get_mut(&file_syncid).unwrap().push(pbs);
            } else {
                files_for_id.insert(file_syncid,vec![pbs]);
            }
        };

        let d = &state.conf.sync_dir();
        let dp = Path::new(d);
        let res = util::visit_dirs(&dp, &mut visitor);
        match res {
            Ok(_) => (),
            Err(e) => panic!("failed to scan directory: {}: {}", d, e),
        }
    }
    files_for_id
}

fn load_syncfile_or_panic(state:&SyncState,syncpath:&String,data:&mut Vec<u8>) -> syncfile::SyncFile {
    let pb = PathBuf::from(syncpath);
    let mut sf = match syncfile::SyncFile::from_syncfile(&state.conf,&pb) {
        Err(e) => panic!("Failed to read syncfile: {:?}", e),
        Ok(sf) => sf
    };
    match sf.decrypt_to_writer(&state.conf, data) {
        Err(e) => panic!("Error {:?}", e),
        Ok(_) => ()
    }
    sf
}

fn dedup_helper(state:&SyncState,dup_cand_idx:usize, paths:&Vec<String>) -> Vec<String> {
    // partition into dups and non dups
    let mut nondups:Vec<String> = Vec::new();
    let mut dups:Vec<(syncfile::SyncFile,String)> = Vec::new();

    let candidate = &paths[dup_cand_idx];
    //println!("dup cand: {}; idx {}, paths: {:?}",candidate,dup_cand_idx,paths);

    let mut cand_data:Vec<u8> = Vec::new();
    let cand_sf = load_syncfile_or_panic(state,&candidate,&mut cand_data);

    for i in 0 .. paths.len() {
        if i < dup_cand_idx {
            nondups.push(paths[i].clone());
        } else if i > dup_cand_idx {
            let mut pot_dup_data:Vec<u8> = Vec::new();
            let pot_dup_sf = load_syncfile_or_panic(state,&paths[i],&mut pot_dup_data);
            if pot_dup_data == cand_data {
                dups.push((pot_dup_sf,paths[i].clone()));
            } else {
                nondups.push(paths[i].clone());
            }
        }
    }

    let mut paths = nondups;

    if !dups.is_empty() {
        // need to find the file with lowest revguid (including the candidate), so
        // push it on to the dup list
        dups.push((cand_sf,candidate.clone()));

        dups.sort_by(|a,b| {
            let asf = &a.0;
            let bsf = &b.0;
            let ord = bsf.revguid.to_string().cmp(&asf.revguid.to_string());
            if ord == Ordering::Equal {
                let afn = &a.1;
                let bfn = &b.1;
                bfn.cmp(afn)
            } else {
                ord
            }
        });

        // dups[0] is the survivor
        let syncpath = &dups[0].1;
        paths.insert(dup_cand_idx, syncpath.clone());
    } else {
        paths.insert(dup_cand_idx, candidate.clone());
    }

    if !dups.is_empty() {
        // println!("for candidate: {}",candidate);
        // println!(" will use: {}", dups[0].1);
        // println!(" and remove:");
        for i in 1 .. dups.len() {
            // println!("   {}", dups[i].1);
            let dup = dups[i].1.clone();

            let pb = PathBuf::from(&dup);
            let pb_par = pb.parent().unwrap();
            let dname = pb_par.to_str().unwrap();

            info!("Removing dup file: {}", dup);
            match remove_file(&dup) {
                Err(e) => warn!("Failed to remove dup sync file: {}: {}", dup, e),
                Ok(_) => ()
            }

            if pb_par.is_dir() {
                match read_dir(dname) {
                    Err(_) => (),
                    Ok(contents) => {
                        let count = contents.count();
                        if count == 0 {
                            //println!("removing empty dir: {}", dname);
                            let _ = remove_dir(dname);
                        }
                    }
                }
            }
        }
    }

    paths.clone()
}

pub fn dedup_syncfiles(state:&mut SyncState) {
    let mut files_for_id = find_all_syncfiles(state);

    // sort by id for consistent ordering
    let mut sids:Vec<String> = Vec::new();
    for (k,_) in &files_for_id {
        sids.push(k.to_string());
    }
    sids.sort();

    //let rem_sync_dir_prefix = |x: &String| { x.to_string().replace(&state.conf.sync_dir, "").to_string() };

    for sid in &sids {
        let files = files_for_id.get_mut(sid).unwrap();

        // get rid of any sync files that are for ignored native files
        let mut valid_files:Vec<String> = Vec::new();
        for sfname in files.iter() {
            let pb = PathBuf::from(&sfname);
            let sf = state.sync_file_cache.get(&state.conf,&pb);
            if !is_ignored(&sf.nativefile) {
                valid_files.push(sfname.clone());
            } else {
                info!("Removing syncfile for ignored native file: {:?}", &sf.nativefile);
                match remove_file(sfname) {
                    Err(e) => panic!("Failed to remove syncfile: {:?}", e),
                    Ok(_) => ()
                }
            }
        }
        files.clear();
        files.append(&mut valid_files);

        if files.len() > 1 {
            // for each file, locate all other duplicates of that file in the list.
            // keep the file with the lowest (numeric) revguid, remove the others.
            // if there are more than one file with the lowest revguid, remove all but one of them.
            //println!("Dup files: {:?}",files);
            let mut dup_cand_idx = 0;
            let mut deduped = files.clone();
            // TODO: figure out how to do this with all the nasty copying, while
            // keeping BC happy
            while dup_cand_idx < deduped.len() {
                // println!("checking dups for: idx: {}: {}", dup_cand_idx, rem_sync_dir_prefix(&deduped[dup_cand_idx]));
                // for x in &deduped {
                //     println!("  pot dup: {}", rem_sync_dir_prefix(&x));
                // }
                let mut reslist = dedup_helper(&state, dup_cand_idx, &mut deduped);
                // println!("res:");
                // for x in &reslist {
                //     println!("  {}", rem_sync_dir_prefix(&x));
                // }
                deduped.clear();
                deduped.append(&mut reslist);
                dup_cand_idx = dup_cand_idx + 1;
            }

            files.clear();
            files.append(&mut deduped);

            if files.len() == 1 {
                // for any non-conflicting sid (i.e only one file), we need to update the syncdb,
                // because the dedup may have changed the active revguid
                let pb = PathBuf::from(&files[0]);
                let sf = state.sync_file_cache.get(&state.conf,&pb);
                let (do_update,mtime) = {
                    match state.syncdb.get(&sf) {
                        Some(entry) => {
                            if sf.revguid != entry.revguid {
                                (true,entry.native_mtime)
                            } else {
                                (false,0)
                            }
                        },
                        None => (false,0), // haven't synced it yet, so this is ok
                    }
                };

                if do_update {
                    // just reuse the mtime, the sf has the latest revguid already, so just update
                    match state.syncdb.update(&sf,mtime) {
                        Err(e) => panic!("Failed to update sync db after dedup: {:?}", e),
                        Ok(_) => {
                            info!("Changed sync revguid for {}", &files[0]);
                        }
                    }
                }
            } else {
                warn!("conflicts: {}", sid)
            }
        }

        //println!("files for {}: {:?}", sid, files);
    }
}

fn is_ignored(f:&str) -> bool {
    let global_ignore = vec![
        glob::Pattern::new("**/.DS_Store").unwrap(), // for a fun time click here: https://github.com/search?utf8=%E2%9C%93&q=.DS_Store&ref=simplesearch
        glob::Pattern::new("**/Thumbs.db").unwrap(), // windows turd
        ];
    for pat in &global_ignore {
        if pat.matches(f) {
            //println!("ignoring: {:?}", pb);
            return true;
        }
    }
    false
}

// Scan the collection of all discovered sync files, and filter out those that
// can be disqualified (conflicted, not mapped, etc).
// Print a message for each rejected file, and return the list of valid files.
fn filter_syncfiles(state:&mut SyncState) -> Vec<String> {
    let mut sync_files:Vec<String> = Vec::new();
    for (sid,files) in &state.sync_files_for_id {
        // don't process conflicts
        if state.is_conflicted(sid) {
            warn!("Ignoring conflicted sync file: {}", files[0]);
            continue;
        }

        let syncfile = files[0].to_string();

        // don't process syncfiles that use a relpath that isn't explicitly specified as a native
        // path on this machine.  This allows the user to skip unpacking a set of files on a given machine.
        let pb = PathBuf::from(&syncfile);
        let sf = state.sync_file_cache.get(&state.conf,&pb);

        let mapping = &state.conf.mapping;

        match state.conf.native_paths.iter().find(|np| {
            let res = mapping.get_kw_relpath(np);
            let (_,nat_relpath) = match res {
                None => return false,
                Some(stuff) => stuff
            };

            //println!("np {:?} rp {:?}", nat_relpath, sf.relpath);
            sf.relpath.starts_with(&nat_relpath)
        }) {
            None => { 
                state.log_util.warn_once(&format!("Ignoring sync file, path not specified as native on this machine: {} (sid: {})", sf.relpath, sf.id));
                continue;
            }
            Some (_) => ()
        }

        sync_files.push(syncfile);
    }
    sync_files
}

pub fn do_sync(state:&mut SyncState) {
    state.sync_file_cache.flush();

    dedup_syncfiles(state);

    state.sync_files_for_id = find_all_syncfiles(state);

    let native_files = {
        // use hashset for path de-dup (TODO: but what about case differences?)
        let mut native_files = HashSet::new();
        {
            let mut visitor = |pb: &PathBuf| {
                let fname = pb.to_str().unwrap().to_owned();
                if !is_ignored(&fname) {
                    native_files.insert(fname);
                }
            };

            let native_paths = &state.conf.native_paths;
            for p in native_paths {
                let pp = PathBuf::from(p);
                if !pp.exists() {
                    warn!("Path does not exist: {}", p);
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

    let mut actions:HashMap<String,SyncAction> = HashMap::new();

    // scan native files
    for nf in &native_files {
        //println!("native file: {}", nf);
        let (sid,syncfile) = match syncfile::SyncFile::get_sync_id_and_path(&state.conf,&nf) {
            Err(e) => {
                warn!("Ignoring native file: {}: {}", &nf, &e);
                continue
            },
            Ok(pair) => pair
        };

        if actions.contains_key(&sid) {
            panic!("Unexpected error: action already present for file: {}", nf)
        }

        // if its conflicted, skip
        if state.is_conflicted(&sid) {
            let cflicts = state.sync_files_for_id.get(&sid).unwrap();
            warn!("Skipping conflicted native file: {}:", nf);
            for c in cflicts {
                warn!("   {  }", c);
            }
            continue
        }

        // the syncfile name may have been remapped, check state
        let syncfile = {
            match state.sync_files_for_id.get(&sid) {
                None => {
                    trace!("name not remapped for native file {}, sid {}", &nf, &sid); 
                    syncfile
                },
                Some (filelist) => PathBuf::from(&filelist[0])
            }
        };

        let np = Some(PathBuf::from(&nf));
        let sd = SyncData { syncid: sid.to_string(), syncfile: syncfile.clone(), nativefile: np };
        if syncfile.is_file() {
            //println!("css for nf: {}: {}", nf, sid);
            actions.insert(sid.to_string(), SyncAction::CompareSyncState(sd));
        } else {
            //println!("usf for nf: {}: {}", nf, sid);
            actions.insert(sid.to_string(), SyncAction::UpdateSyncfile(sd));
        }
    }

    // scan sync files
    let sync_files:Vec<String> = filter_syncfiles(state);

    for sf in &sync_files {
        let syncfile = PathBuf::from(sf);
        let sid = match syncfile::SyncFile::get_syncid_from_file(&syncfile) {
            Err(e) => panic!("Can't get syncid from file: {}", e),
            Ok(sid) => sid
        };

        // if we already have a compare action pending for the file, we don't need to crack it
        {
            let action = actions.get(&sid);
            match action {
                None => (),
                Some(action) => {
                    match *action {
                        SyncAction::CheckFilesEqualElseConflict(_)
                        | SyncAction::CompareSyncState(_) => continue, // skip
                        SyncAction::CheckSyncRevguid(_) => panic!("Check sync revguid shouldn't be here"),
                        SyncAction::CreateNewNativeFile(_) => panic!("Create new native file shouldn't be here"),
                        SyncAction::ProcessNativeDelete(_) => panic!("Process native delete shouldn't be here"),
                        SyncAction::ProcessSyncfileDelete(_) => panic!("Process sync delete shouldn't be here"),
                        SyncAction::Nothing => (),
                        SyncAction::UpdateNativeFile(_)
                        | SyncAction::UpdateSyncfile(_) =>
                            // this probably just becomes a compare, but we should have detected it earlier
                            panic!("Already have update pending for native file")
                    }
                }
            }
        }

        // no action yet, so we did not scan a native file that maps to the same sid.  have to check
        // revguids to see what to do.
        let sd = SyncData { syncid: sid.to_string(), syncfile: syncfile.clone(), nativefile: None };
        actions.insert(sid.to_string(), SyncAction::CheckSyncRevguid(sd));
    }

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

    for (sid,action) in actions {
        match action {
            SyncAction::Nothing => (),
            _ => error!("Leftover action in list: {:?} for {:?}", action, sid)
        }
    }
}

#[cfg(test)]
mod tests {
    // Ok, here were gonna test ALL of the core sync functionality.
    // HAHA just kidding.  But we'll get a lot of it.  This is an 
    // integration test rather than a strict unit test.
    
    // The basic idea is to create a fake sync situation, with two
    // actors, Alice and Bob, that are both configured to use the same
    // syncdir and similar native paths.  Then we test various perumations of syncing;
    // Create some native files as Alice, run her Sync, run Bob's sync,
    // does he get them?  Repeat for every interesting test case.  
    //
    // Alice and Bob share a syncdir, but they must have different syncdbs and
    // local path directories, otherwise they will step on each another, a situation
    // that is prevented in the real world by the process/syncdir mutexes.  Actually, 
    // even sharing a syncdir doesn't map to the real world, because in the RW
    // the sync dir is only virtually the same; e.g in google drive, to processes 
    // that "simultaneously" (for some definition) write the same file to the directory
    // will cause that system to generate two different files with different names;
    // (the dreaded Foo (1).txt situation). 
    //
    // But I digress.  Back to directories.  Actually, each _test_ will need to have 
    // its own test-specific directory structure, because the cargo test harness runs them all in 
    // parallel - which is good, I don't want to be waiting around for the tests.
    // Since all are in parallel, the tests
    // will stomp each other if using same directories.  So we create a directory set
    // for each test, by name.  This has the added virtue that if a test fails, we
    // can inspect the output directory for that test postmortem.
    //
    // Another way that these tests deviate from the RW is that we run syncs serially,
    // but in the RW of course, both greycrypt instances and the cloud provider run 
    // in parallel.  In the future in may be useful to add more coverage for those cases.

    
    use std::path::{PathBuf};
    use std::env;
    use std::fs::{create_dir_all,remove_dir_all,PathExt,copy};
    
    extern crate toml;
    
    use core;
    use config;
    use logging;
    use mapping;
    use testlib;
    use syncdb;
    
    struct TestDirectories {
        sync_dir: String,
        alice_syncdb: String,
        alice_native: String,
        bob_syncdb: String,
        bob_native: String
    }

    // Clean out (remove) and recreate directories required for the target test.  
    // This function operates on $wd/testdata/out_core/<testname>_<dirtype> directories
    // only.  It returns a struct of all the dir names.     
    fn init_test_directories(testname:&str) -> TestDirectories {   
        let recycle_test_dir = |relpath:&str| {
            let wd = env::current_dir().unwrap();
            let mut path = PathBuf::from(&wd);
            path.push("testdata");
            path.push("out_core");
            if testname.contains("..") {
                panic!("illegal testname, '..' not allowed: {}", testname);
            }
            path.push(testname);        
            if relpath.contains("..") {
                panic!("illegal relpath, '..' not allowed: {}", relpath);
            }            
            path.push(relpath);
            
            let path_str = path.to_str().unwrap();
            if path.is_dir() {
                //println!("would remove {:?}", path);
                match remove_dir_all(&path) {
                    Err(e) => panic!("Failed to remove test output directory: {:?}: {:?}", path_str, e),
                    Ok(_) => ()
                }
            }
            match create_dir_all(&path_str) {
                Err(e) => panic!("Failed to create test output directory: {:?}: {:?}", path_str, e),
                Ok(_) => ()
            }
            
            path_str.to_owned()            
        };
        
        TestDirectories {
            sync_dir: recycle_test_dir("syncdir.lastrun"),
            alice_syncdb: recycle_test_dir("syncdb.alice.lastrun"), 
            alice_native: recycle_test_dir("native.alice.lastrun"),
            bob_syncdb: recycle_test_dir("syncdb.bob.lastrun"),
            bob_native: recycle_test_dir("native.bob.lastrun") 
        }
    }
    
    // This struct contains a normal sync state, as well as additional data useful for the
    // unit tests.
    //#[derive(Debug)]
    struct MetaConfig {
        native_root: String,
        state: core::SyncState,
    }
    
	fn get_meta_config(native_dir:&str, syncdb_dir:&str, sync_dir:&str, log_util:logging::LoggerUtil) -> MetaConfig {       
        let mapping = format!("home = '{}'", native_dir);
        let mapping = toml::Parser::new(&mapping).parse().unwrap();
        let mapping = mapping::Mapping::new(&mapping).ok().expect("WTF?");

        let ec: [u8;config::KEY_SIZE] = [0; config::KEY_SIZE];

        let conf = config::SyncConfig::new(
            sync_dir.to_owned(),
            testlib::util::unit_test_hostname(),
            mapping,
            Some(ec),
            Some(syncdb_dir.to_owned()),
            Vec::new());
            
        let syncdb = match syncdb::SyncDb::new(&conf) {
            Err(e) => panic!("Failed to create syncdb: {:?}", e),
            Ok(sdb) => sdb
        };
        
        let state = core::SyncState::new(conf,syncdb,log_util);
                      
        MetaConfig {
            native_root: native_dir.to_owned(),
            state: state
        }
    }    
    
    fn config_alice_and_bob(dirs:&TestDirectories) -> (MetaConfig, MetaConfig) {
        let log_util = match logging::SimpleLogger::init() {
            Err(e) => panic!("Failed to init logger: {}", e),
            Ok(l) => l
        };
        
        let alicec = get_meta_config(&dirs.alice_native, &dirs.alice_syncdb, &dirs.sync_dir, log_util.clone());
        let bobc = get_meta_config(&dirs.bob_native, &dirs.bob_syncdb, &dirs.sync_dir, log_util.clone());
        (alicec,bobc)
    }
    
    fn populate_native(target_native_dir:&str,subdir: Option<&str>) {
        let cp_or_panic = |src:&str,dest:&PathBuf| {
            let mut dest = dest.clone();
            let srcpath = PathBuf::from(src);
            if dest.is_dir() {
                dest.push(srcpath.file_name().unwrap());
            }
            //println!("cp: {:?} -> {:?}", srcpath, dest);
            match copy(src,dest.to_str().unwrap()) {
                Err(e) => panic!("Failed to copy test native file {} to {:?}: {}", src, dest, e),
                Ok(_) => () 
            } 
        };
        
        let outpath = {
            let mut outpath = PathBuf::from(target_native_dir);
            match subdir {
                None => outpath,
                Some (subdir) => {
                    outpath.push(subdir);
                    create_dir_all(outpath.to_str().unwrap()).unwrap();
                    outpath
                }
            }
        };
        
        cp_or_panic("testdata/test_text_file.txt", &outpath);
        cp_or_panic("testdata/test_binary.png", &outpath);    
    }
    
    fn add_native_path(mconf: &mut MetaConfig, path: &str) {
        let mut pb = PathBuf::from(&mconf.native_root);
        pb.push(path);
        mconf.state.conf.native_paths.push(pb.to_str().unwrap().to_owned());
    }

    #[test]
    fn basic_sync() {
        // run a sync on alice
        // verify sync state for alice (see below)
        // run a sync state for bob
        // verify sync state for bob
        
        // verify sync state:
        //  the number of files in the native directory == the number of files in the sync dir
        //  the decrypted contents of each sync file match the contents in the native directory
        //  the revguid for each syncfile matches the revguid in the syncdb
        //  the mtime for each native file matches the mtime in the syncdb
    
        let dirs = init_test_directories("basic_sync");
        let (ref mut alice_mconf, ref mut bob_mconf) = config_alice_and_bob(&dirs);
        
        // populate alice's native directory
        populate_native(&dirs.alice_native, Some("docs"));
        // map the path in both configs
        add_native_path(alice_mconf, "docs");
        add_native_path(bob_mconf, "docs");
        
        // sync alice
        core::do_sync(&mut alice_mconf.state);
        
        // TODO: test alice state
        
        // sync bob
        core::do_sync(&mut bob_mconf.state);
        
        // TODO: test bob state
    }
}
