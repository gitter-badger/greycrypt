#[cfg(test)]
pub mod util {
    use std::env;
    use std::fs::{File,create_dir_all,remove_dir_all,PathExt,copy,remove_file};
    use std::io::Write;
    use std::path::{Path,PathBuf};
    
    extern crate toml;

	use config;
    use core;
    use logging;
    use mapping;
    use syncdb;
    use syncfile;
    use util;

	#[cfg(target_os = "windows")]
	pub fn unit_test_hostname() -> String {
		"WinUnitTestHost".to_owned()
	}

	#[cfg(target_os = "macos")]
	pub fn unit_test_hostname() -> String {
		"MacUnitTestHost".to_owned()
	}

    pub fn clear_test_syncdb(conf:&config::SyncConfig) {
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

	pub fn get_mock_config() -> config::SyncConfig {
        let wd = env::current_dir().unwrap();

        // generate a mock mapping, with keyword "gcprojroot" mapped to the project's root dir
        let wds = wd.to_str().unwrap();
        let mapping = format!("gcprojroot = '{}'", wds);
        let mapping = toml::Parser::new(&mapping).parse().unwrap();
        let mapping = mapping::Mapping::new(&mapping).ok().expect("WTF?");

        let mut outpath = PathBuf::from(&wd);
        outpath.push("testdata");
        outpath.push("out_syncdir");

        let mut syncdb_dir = PathBuf::from(&wd);
        syncdb_dir.push("testdata");
        syncdb_dir.push("out_syncdb");

        let ek: [u8;config::KEY_SIZE] = [0; config::KEY_SIZE];

        let conf = config::SyncConfig::new(
            outpath.to_str().unwrap().to_owned(),
            unit_test_hostname(),
            mapping,
            Some(ek),
            Some(syncdb_dir.to_str().unwrap().to_owned()),
            Vec::new());
        conf
    }

    pub fn get_test_config() -> config::SyncConfig {
        let wd = env::current_dir().unwrap();
        let mut syncpath = PathBuf::from(&wd);
        syncpath.push("testdata");
        syncpath.push("test_config.toml");
        let config_file = syncpath.to_str().unwrap().to_owned();
        let config = config::parse(Some(config_file),Some(unit_test_hostname()), None);
        config
    }
    
    pub struct TestDirectories {
        sync_dir: String,
        alice_syncdb: String,
        alice_native: String,
        bob_syncdb: String,
        bob_native: String
    }

    // Clean out (remove) and recreate directories required for the target test.
    // This function operates on $wd/testdata/out_core/<testname>_<dirtype> directories
    // only.  It returns a struct of all the dir names.
    pub fn init_test_directories(testname:&str) -> TestDirectories {
        let recycle_test_dir = |relpath:&str| {
            let wd = env::current_dir().unwrap();
            let mut path = PathBuf::from(&wd);
            path.push("testdata");
            if testname.contains("..") {
                panic!("illegal testname, '..' not allowed: {}", testname);
            }
            path.push(format!("out_core_{}", testname));
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
    pub struct MetaConfig {
        pub native_root: String,
        pub state: core::SyncState,
    }

	pub fn get_meta_config(native_dir:&str, syncdb_dir:&str, sync_dir:&str, log_util:logging::LoggerUtil) -> MetaConfig {
        let mapping = format!("home = '{}'", native_dir);
        let mapping = toml::Parser::new(&mapping).parse().unwrap();
        let mapping = mapping::Mapping::new(&mapping).ok().expect("WTF?");

        let ek: [u8;config::KEY_SIZE] = [0; config::KEY_SIZE];

        let conf = config::SyncConfig::new(
            sync_dir.to_owned(),
            unit_test_hostname(),
            mapping,
            Some(ek),
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

    pub fn config_alice_and_bob(dirs:&TestDirectories) -> (MetaConfig, MetaConfig) {
        let log_util = match logging::init(None) {
            Err(e) => panic!("Failed to init logger: {}", e),
            Ok(l) => l
        };

        let alicec = get_meta_config(&dirs.alice_native, &dirs.alice_syncdb, &dirs.sync_dir, log_util.clone());
        let bobc = get_meta_config(&dirs.bob_native, &dirs.bob_syncdb, &dirs.sync_dir, log_util.clone());
        (alicec,bobc)
    }

    pub fn cp_or_panic(src:&str,dest:&PathBuf) {
        let mut dest = dest.clone();
        let srcpath = PathBuf::from(src);
        if dest.is_dir() {
            dest.push(srcpath.file_name().unwrap());
        }
        //println!("cp: {:?} -> {:?}", srcpath, dest);
        match copy(src,dest.to_str().unwrap()) {
            Err(e) => panic!("Failed to copy test file {} to {:?}: {}", src, dest, e),
            Ok(_) => ()
        }
    }

    pub fn populate_native(target_native_dir:&str,subdir: Option<&str>) {
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

    pub fn add_native_path(mconf: &mut MetaConfig, path: &str) {
        let mut pb = PathBuf::from(&mconf.native_root);
        pb.push(path);
        mconf.state.conf.native_paths.push(pb.to_str().unwrap().to_owned());
    }

    pub fn find_all_files(dir:&str) -> Vec<String> {
        let mut files:Vec<String> = Vec::new();
        {
            let mut visitor = |pb: &PathBuf| files.push(pb.to_str().unwrap().to_owned());

            let dp = Path::new(dir);
            let res = util::visit_dirs(&dp, &mut visitor);
            match res {
                Ok(_) => (),
                Err(e) => panic!("failed to scan directory: {}: {}", dir, e),
            }
        }
        files
    }

    // Verifies that:
    //  the number of files in the native directory == the number of files in the sync dir
    //  the decrypted contents of each sync file match the contents in the native directory
    //  the revguid for each syncfile matches the revguid in the syncdb
    //  the mtime for each native file matches the mtime in the syncdb
    pub fn verify_sync_state(mconf: &mut MetaConfig, expected_syncfiles: usize, expected_nativefiles: usize) {
        // find all the syncfiles
        let syncfiles = find_all_files(mconf.state.conf.sync_dir());
        // verify that the number found == expected
        assert_eq!(syncfiles.len(), expected_syncfiles);

        // reload the syncdb off disk, so that we can check both the one in state and the
        // one on disk
        let mut disk_syncdb = match syncdb::SyncDb::new(&mconf.state.conf) {
            Err(e) => panic!("Failed to create syncdb: {:?}", e),
            Ok(sdb) => sdb
        };

        let verify_sync_entry = |syncdb: &mut syncdb::SyncDb, sf: &syncfile::SyncFile| {
            let entry = syncdb.get(sf);
            match entry {
                None => panic!("Syncdb should have an entry, but has none"),
                Some(entry) => {
                    assert_eq!(entry.revguid, sf.revguid);

                    if sf.is_deleted {
                        assert_eq!(entry.native_mtime, 0);
                    } else {
                        let nmtime = match util::get_file_mtime(&sf.nativefile) {
                            Err(e) => panic!("Whoa should have an mtime: {}", e),
                            Ok(nmtime) => nmtime
                        };
                        assert_eq!(entry.native_mtime, nmtime);
                    }
                }
            }
        };

        // for each syncfile...
        for syncpath in &syncfiles {
            let syncpath = PathBuf::from(&syncpath);

            // decrypt to mem
            let mut sf = match syncfile::SyncFile::from_syncfile(&mconf.state.conf,&syncpath) {
                Err(e) => panic!("Failed to read syncfile: {:?}", e),
                Ok(sf) => sf
            };

            let mut data:Vec<u8> = Vec::new();

            match sf.decrypt_to_writer(&mconf.state.conf, &mut data) {
                Err(e) => panic!("Error {:?}", e),
                Ok(_) => {

                    // verify that native file exists and has same contents
                    let nf = PathBuf::from(&sf.nativefile);
                    if sf.is_deleted {
                        assert!(!nf.is_file());
                        assert_eq!(data.len(),0);
                    } else {
                        //println!("nf: {}", &sf.nativefile);
                        assert!(nf.is_file());
                        // suck it up
                        let ndata = util::slurp_bin_file(&sf.nativefile);
                        // verify
                        assert_eq!(data,ndata);
                    }
                }
            }

            // check revguid, mtime
            verify_sync_entry(&mut mconf.state.syncdb, &sf);
            verify_sync_entry(&mut disk_syncdb, &sf);
        }

        // find all the native files
        let nfiles = find_all_files(&mconf.native_root);
        // verify that the number found == expected
        assert_eq!(nfiles.len(), expected_nativefiles);
    }

    pub fn basic_alice_bob_setup(testname:&str) -> (MetaConfig, MetaConfig) {
        let dirs = init_test_directories(testname);
        let (mut alice_mconf, mut bob_mconf) = config_alice_and_bob(&dirs);
    
        // populate alice's native directory
        populate_native(&dirs.alice_native, Some("docs"));
        // map the path in both configs
        add_native_path(&mut alice_mconf, "docs");
        add_native_path(&mut bob_mconf, "docs");
    
        (alice_mconf, bob_mconf)
    }
    
    pub fn write_text_file(fpath:&str, text:&str) {
        match File::create(fpath) {
            Err(e) => panic!("{}", e),
            Ok(ref mut f) => {
                match f.write_all(text.as_bytes()) {
                    Err(e) => panic!("{}", e),
                    Ok(_) => ()
                }
            }
        };
    }

    pub fn delete_text_file(mconf:&MetaConfig) {
        let mut text_pb = PathBuf::from(&mconf.native_root);
        text_pb.push("docs");
        let mut out1 = text_pb.clone();
        out1.push("test_text_file.txt");
    
        // delete
        match remove_file(out1.to_str().unwrap()) {
            Err(e) => panic!("{}", e),
            Ok(_) => ()
        }
    }
    
    pub fn update_text_file(mconf:&MetaConfig,newtext:&str) {
        let mut text_pb = PathBuf::from(&mconf.native_root);
        text_pb.push("docs");
        let mut out1 = text_pb.clone();
        out1.push("test_text_file.txt");
        write_text_file(out1.to_str().unwrap(), newtext);
    }
}
