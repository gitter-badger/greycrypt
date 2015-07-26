#[cfg(test)]
pub mod util {
    use std::env;
    use std::path::{PathBuf};
    
    extern crate toml;
    
	use config;
    use mapping;
	
	#[cfg(target_os = "windows")]
	pub fn unit_test_hostname() -> String {
		"WinUnitTestHost".to_owned()
	}
	
	#[cfg(target_os = "macos")]
	pub fn unit_test_hostname() -> String {
		"MacUnitTestHost".to_owned()
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

        let ec: [u8;config::KEY_SIZE] = [0; config::KEY_SIZE];

        let conf = config::SyncConfig::new(
            outpath.to_str().unwrap().to_owned(),
            unit_test_hostname(),
            mapping,
            Some(ec),
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
        let config = config::parse(Some(config_file));
        config::SyncConfig::new(
            config.sync_dir().to_owned(),
            unit_test_hostname(),
            config.mapping,
            config.encryption_key,
            config.syncdb_dir,
            config.native_paths
        )    
    }
}
