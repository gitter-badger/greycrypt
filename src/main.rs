#![feature(path_ext)]
use std::fs::{PathExt};
use std::path::{Path,PathBuf};
use std::collections::HashSet;

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

fn compare_sync_state(conf:&config::SyncConfig,sd:&SyncData) -> SyncAction {
    println!("Comparing file times on: {:?} and {:?}", sd.nativefile.file_name().unwrap(), sd.syncfile.file_name().unwrap());
    SyncAction::Nothing
}
fn update_sync_file(conf:&config::SyncConfig,sd:&SyncData) -> SyncAction {
    println!("Copying native data in {:?} to {:?}", sd.nativefile.file_name().unwrap(), sd.syncfile.file_name().unwrap());
    let sfpath = match syncfile::SyncFile::create_syncfile(&conf,&sd.nativefile) {
        Err(e) => panic!("Error creating sync file: {:?}", e),
        Ok(sfpath) => sfpath
    };

    SyncAction::Nothing
}

fn pass1_prep(conf:&config::SyncConfig,sa:SyncAction) -> SyncAction {
    match sa {
        SyncAction::Nothing => sa,
        SyncAction::CompareSyncState(ref sd) => compare_sync_state(conf,sd),
        SyncAction::UpdateSyncfile(_) => sa, // don't do this in pass1
    }
}
fn pass2_verify(conf:&config::SyncConfig,sa:SyncAction) -> SyncAction {
    match sa {
        SyncAction::Nothing => sa,
        SyncAction::CompareSyncState(ref sd) => panic!("Cannot compare sync state here"),
        SyncAction::UpdateSyncfile(_) => sa,
    }
}
fn pass3_commit(conf:&config::SyncConfig,sa:SyncAction) -> SyncAction {
    match sa {
        SyncAction::Nothing => sa,
        SyncAction::CompareSyncState(ref sd) => panic!("Cannot compare sync state here"),
        SyncAction::UpdateSyncfile(ref sd) => update_sync_file(conf,sd),
    }
}

fn do_sync(conf:&config::SyncConfig) {
    let native_paths = &conf.native_paths;

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
                let res = util::visit_dirs(pp.as_path(), &mut visitor);
                match res {
                    Ok(_) => (),
                    Err(e) => panic!("failed to scan directory: {}: {}", pp.to_str().unwrap(), e),
                }
            }
        };
    }

    let mut actions:Vec<SyncAction> = Vec::new();

    for nf in &native_files {
        //println!("native file: {}", nf);
        let (sid,syncfile) = match syncfile::SyncFile::get_sync_id_and_path(&conf,&nf) {
            Err(e) => {
                println!("Ignoring native file: {}: {}", &nf, &e);
                continue
            },
            Ok(pair) => pair
        };

        let np = PathBuf::from(&nf);
        let sd = SyncData { syncid: sid, syncfile: syncfile.clone(), nativefile: np };
        if syncfile.is_file() {
            actions.push(SyncAction::CompareSyncState(sd))
        } else {
            actions.push(SyncAction::UpdateSyncfile(sd))
        }
    }

    // TODO: use map() once I figure how to match on the struct references
    //let actions = actions.iter().map(|a| pass1_action_handler(a) );
    let actions = {
        let mut new_actions:Vec<SyncAction> = Vec::new();
        for a in actions {
            new_actions.push(pass1_prep(conf,a));
        }
        new_actions
    };
    let actions = {
        let mut new_actions:Vec<SyncAction> = Vec::new();
        for a in actions {
            new_actions.push(pass2_verify(conf,a));
        }
        new_actions
    };
    let actions = {
        let mut new_actions:Vec<SyncAction> = Vec::new();
        for a in actions {
            new_actions.push(pass3_commit(conf,a));
        }
        new_actions
    };

}

fn main() {
    let conf = config::parse();
    do_sync(&conf);
}
