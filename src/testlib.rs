#[cfg(test)]
pub mod util {
    use std::env;
    use std::path::{PathBuf};
    use std::fs::{PathExt,remove_dir_all};

    extern crate toml;

	use config;
    use mapping;
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
        let config = config::parse(Some(config_file),Some(unit_test_hostname()));
        config
    }
}
