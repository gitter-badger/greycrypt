use std::result;

extern crate uuid;

use mapping;

pub struct SyncFile {
    id: String,
    revguid: uuid::Uuid
}

impl SyncFile {
    pub fn from_native(mapping: &mapping::Mapping, nativepath: &str) -> Result<SyncFile,String> {
        Err("my shit not implemented".to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::path::{PathBuf};
    use mapping;

    extern crate toml;

    #[test]
    fn create_syncfile() {
        //let sf = syncfile::SyncFile::from_native("");
        let wd = env::current_dir().unwrap();

        // generate a mock mapping
        let wds = wd.to_str();
        let mapping = format!("gcprojroot = '{}'", wds.unwrap());
        let mapping = toml::Parser::new(&mapping).parse().unwrap();
        let mapping = mapping::Mapping::new(&mapping).ok().expect("WTF?");

        let mut testpath = PathBuf::from(&wd);
        testpath.push("testdata");
        testpath.push("test_native_file.txt");
        let res = mapping.get_kw_relpath(testpath.to_str().unwrap());
        println!("kw {:?}", res);
        // //let tf = format!("{}{})
        // println!("{:?}", testpath);
        assert!(true);
    }
}
