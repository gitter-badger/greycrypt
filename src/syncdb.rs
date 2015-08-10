extern crate uuid;

use std::io::Write;
use std::fs::{PathExt};
use std::fs::{File,create_dir_all};
use std::path::{PathBuf};
use std::collections::HashMap;

use util;
use config;
use syncfile;

pub struct SyncEntry {
    pub revguid: uuid::Uuid,
    pub native_mtime: u64
}

pub struct SyncDb {
    syncdb_dir: PathBuf,
    cache: HashMap<String,SyncEntry>
}

impl SyncDb {
    pub fn new(conf: &config::SyncConfig) -> Result<Self,String> {
        // if conf has a db dir, use that; otherwise, form it from the app data path
        let syncdb_dir = {
            match conf.syncdb_dir {
                Some(ref dir) => PathBuf::from(&dir.to_owned()),
                None => {
                    let ad_dir = util::get_appdata_dir();
                    match ad_dir {
                        None => return Err("No appdata dir available, can't initialize syncdb".to_owned()),
                        Some(dir) => {
                            // append app name
                            let mut pb = PathBuf::from(&dir);
                            pb.push("GreyCrypt");
                            pb.push(config::BUILD_PREFIX);
                            pb
                        }
                    }
                }
            }
        };

        if !syncdb_dir.is_dir() {
            let res = create_dir_all(&syncdb_dir);
            match res {
                Err(e) => return Err(format!("Failed to create syncdb directory: {:?}: {:?}", syncdb_dir, e)),
                Ok(_) => ()
            }
        }

        let res = SyncDb {
            syncdb_dir: syncdb_dir,
            cache: HashMap::new()
        };

        Ok(res)
    }

    fn get_store_path(&self,sid:&str) -> PathBuf {
        let mut pentry = self.syncdb_dir.clone();
        let prefix = &sid[0..2];
        pentry.push(&prefix);
        pentry.push(&sid);
        pentry
    }

    pub fn update(&mut self, sf:&syncfile::SyncFile, native_mtime:u64) -> Result<(),String> {
        let entry = SyncEntry {
            revguid: sf.revguid,
            native_mtime: native_mtime
        };

        // write to disk: two-line file
        // should switch to toml if this gets more complicated
        {
            let storepath:PathBuf = self.get_store_path(&sf.id);

            let storepath_par = storepath.parent().unwrap();
            if !storepath_par.is_dir() {
                let res = create_dir_all(&storepath_par);
                match res {
                    Err(e) => return Err(format!("Failed to create syncdb Store directory: {:?}: {:?}", storepath_par, e)),
                    Ok(_) => ()
                }
            }

            let mut f = match File::create(&storepath) {
                Err(e) => return Err(format!("{:?}", e)),
                Ok(f) => f
            };

            match writeln!(f, "revguid: {}", &entry.revguid.to_owned()) {
                Err(e) => return Err(format!("{:?}", e)),
                Ok(_) => ()
            }
            match writeln!(f, "native_mtime: {}", entry.native_mtime) {
                Err(e) => return Err(format!("{:?}", e)),
                Ok(_) => ()
            }
        }

        let _ = self.cache.insert(sf.id.clone(),entry);

        Ok(())
    }
    
    
    pub fn get(&mut self, sf:&syncfile::SyncFile) -> Option<&SyncEntry> {
        self.get_by_sid(&sf.id)
    }
    
    pub fn get_by_sid(&mut self, sid: &str) -> Option<&SyncEntry> {
        // can't just get() the key here, because that will introduce an
        // immutable borrow on the map, and we may need to mutate it to add
        // the entry from disk.
        if self.cache.contains_key(sid) {
            let res = self.cache.get(sid);
            return res;
        } else {
            // lookup in fs
            let storepath = self.get_store_path(&sid);

            if !storepath.is_file() {
                return None
            }

            let entry_text = util::slurp_text_file(&storepath.to_str().unwrap());
            let lines:Vec<&str> = entry_text.lines().collect();
            let hm = util::string_lines_to_hashmap(lines);
            let revguid_str = hm.get("revguid").expect("Need revguid");
            let mtime_str = hm.get("native_mtime").expect("Need native_mtime");

            let revguid = match uuid::Uuid::parse_str(revguid_str) {
                Err(e) => panic!("Couldn't parse UUID str: {:?}", e),
                Ok(id) => id
            };
            let mtime = match u64::from_str_radix(mtime_str, 10) {
                Err(e) => panic!("Couldn't parse mtime str: {:?}", e),
                Ok(mtime) => mtime
            };

            let entry = SyncEntry {
                revguid: revguid,
                native_mtime: mtime
            };

            assert!(!self.cache.contains_key(sid));
            self.cache.insert(sid.to_owned(), entry);
            self.cache.get(sid)
        }
    }

    #[cfg(test)]
    pub fn flush_cache(&mut self) {
        self.cache.clear();
    }
}

#[cfg(test)]
mod tests {
    extern crate toml;

    use std::path::{PathBuf};
    use std::env;

    use util;
    use syncdb;
    use syncfile;
    use testlib;
    
    #[test]
    fn store() {
        let conf = testlib::util::get_mock_config();

        testlib::util::clear_test_syncdb(&conf);

        let mut syncdb = match syncdb::SyncDb::new(&conf) {
            Err(e) => panic!("Failed to create syncdb: {:?}", e),
            Ok(sdb) => sdb
        };

        let wd = env::current_dir().unwrap();
        let mut nativepath = PathBuf::from(&wd);
        nativepath.push("testdata");
        nativepath.push("test_text_file.txt");

        let mut syncpath = PathBuf::from(&wd);
        syncpath.push("testdata");
        syncpath.push("d759e740d8ecef87b9aa331b1e5edc3aeed133d51347beed735a802253b775b5.dat");

        let mtime = match util::get_file_mtime(&nativepath.to_str().unwrap()) {
            Err(e) => panic!("Failed to get native mtime: {:?}", e),
            Ok(time) => time
        };
        let sf = match syncfile::SyncFile::from_syncfile(&conf,&syncpath) {
            Err(e) => panic!("Failed to read syncfile: {:?}", e),
            Ok(sf) => sf
        };

        let check_syncdb_empty = |syncdb:&mut syncdb::SyncDb| {
            let entry = syncdb.get(&sf);
            match entry {
                None => (),
                Some(_) => panic!("Syncdb should be empty, but has an entry")
            }
        };
        let check_entry = |entry: &syncdb::SyncEntry | {
            assert_eq!(entry.revguid, sf.revguid );
            assert_eq!(entry.native_mtime, mtime);
        };

        check_syncdb_empty(&mut syncdb);

        match syncdb.update(&sf,mtime) {
            Err(e) => panic!("Failed to update syncdb: {:?}", e),
            Ok(_) => ()
        };

        // entry should now exist
        {
            let entry = syncdb.get(&sf).expect("Expected sync entry");
            check_entry(entry);
        }

        // clear cache
        {
            let mut syncdb:&mut syncdb::SyncDb = &mut syncdb;
            syncdb.flush_cache();
        }

        // entry should still exist, loaded from disk this time
        {
            let entry = syncdb.get(&sf).expect("Expected sync entry");
            check_entry(entry);
        }
    }
}
