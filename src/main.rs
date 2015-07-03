// this is really spammy for me, will have to enable periodically
#![allow(dead_code)]
#![allow(unused_variables)]

#![feature(path_ext)]

#![feature(append)] // for sync dedup, hopefully can remove

mod util;
mod config;
mod mapping;
mod syncfile;
mod crypto_util;
mod syncdb;
mod core;

use std::collections::HashMap;

fn main() {
    let conf = config::parse();
    let syncdb = match syncdb::SyncDb::new(&conf) {
        Err(e) => panic!("Failed to create syncdb: {:?}", e),
        Ok(sdb) => sdb
    };

    let mut state = core::SyncState {
        syncdb: syncdb,
        conf: conf,
        sync_files_for_id: HashMap::new()
    };
    core::do_sync(&mut state);
}
