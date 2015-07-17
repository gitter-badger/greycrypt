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
    pub fn new(conf: &config::SyncConfig) -> Result<SyncDb,String> {
        // if conf has a db dir, use that; otherwise, form it from the app data path
        let syncdb_dir = {
            match conf.syncdb_dir {
                Some(ref dir) => PathBuf::from(&dir.to_string()),
                None => {
                    let ad_dir = util::get_appdata_dir();
                    match ad_dir {
                        None => return Err("No appdata dir available, can't initialize syncdb".to_string()),
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

        // write to disk
        // TODO: should probably use toml for this
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

            match writeln!(f, "revguid: {}", &entry.revguid.to_string()) {
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
        // can't just get() the key here, because that will introduce an
        // immutable borrow on the map, and we may need to mutate it to add
        // the entry from disk.
        if self.cache.contains_key(&sf.id) {
            let res = self.cache.get(&sf.id);
            return res;
        } else {
            // lookup in fs
            let storepath = self.get_store_path(&sf.id);

            if !storepath.is_file() {
                return None
            }

            //let toml = util::load_toml_file(&"mapping.toml".to_string());
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

            assert!(!self.cache.contains_key(&sf.id));
            self.cache.insert(sf.id.to_string(), entry);
            self.cache.get(&sf.id)
        }
    }

    pub fn flush_cache(&mut self) {
        self.cache.clear();
    }
}

#[cfg(test)]
mod tests {
    extern crate toml;

    use std::fs::{PathExt};
    use std::path::{PathBuf};
    use std::env;
    use std::fs::remove_dir_all;

    use util;
    use config;
    use syncdb;
    use mapping;
    use syncfile;

    fn get_config() -> config::SyncConfig {
        let wd = env::current_dir().unwrap();

        // empty mapping for this test
        let mapping = "";
        let mapping = toml::Parser::new(&mapping).parse().unwrap();
        let mapping = mapping::Mapping::new(&mapping).ok().expect("WTF?");

        let mut outpath = PathBuf::from(&wd);
        outpath.push("testdata");
        outpath.push("syncdir");

        let mut syncdb_dir = PathBuf::from(&wd);
        syncdb_dir.push("testdata");
        syncdb_dir.push("out_syncdb");

        let ec: [u8;config::KEY_SIZE] = [0; config::KEY_SIZE];

        let conf = config::SyncConfig::new(
            outpath.to_str().unwrap().to_string(),
            "MacUnitTestHost".to_string(), // TODO: use win on windows
            mapping,
            Some(ec),
            Some(syncdb_dir.to_str().unwrap().to_string()),
            Vec::new());
        conf
    }

    #[test]
    fn store() {
        let conf = get_config();

        {
            // if the previous test sync db exists, clear it out
            // TODO: should break this out into test lib function for reuse
            let wd = env::current_dir().unwrap();
            let sdb_path = conf.syncdb_dir.clone().unwrap();
            let sdb_path = PathBuf::from(sdb_path);
            if sdb_path.is_dir() {
                let sdb_path_str = sdb_path.to_str().unwrap();
                if !util::canon_path(sdb_path_str).contains("testdata/out_syncdb") ||
                    !sdb_path_str.starts_with(wd.to_str().unwrap()) ||
                    sdb_path_str.contains("..") {
                    panic!("Refusing to remove this unrecognized test syncdb: {:?}", sdb_path)
                } else {
                    match remove_dir_all(sdb_path_str) {
                        Err(e) => panic!("Failed to remove previous output syncdb: {:?}: {:?}", sdb_path_str, e),
                        Ok(_) => ()
                    }
                }
            }
        }

        let mut syncdb = match syncdb::SyncDb::new(&conf) {
            Err(e) => panic!("Failed to create syncdb: {:?}", e),
            Ok(sdb) => sdb
        };

        let wd = env::current_dir().unwrap();
        let mut nativepath = PathBuf::from(&wd);
        nativepath.push("testdata");
        nativepath.push("test_native_file.txt");

        let mut syncpath = PathBuf::from(&wd);
        syncpath.push("testdata");
        syncpath.push("6539709be17615dbbf5d55f84f293c55ecc50abf4865374c916bef052e713fec.dat");

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
