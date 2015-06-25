use std::result;

extern crate uuid;

pub struct SyncFile {
    id: String,
    revguid: uuid::Uuid
}

impl SyncFile {
    pub fn from_native(nativefile: &str) -> Result<SyncFile,String> {
        Err("my shit not implemented".to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::path::{PathBuf};

    #[test]
    fn create_syncfile() {
        //let sf = syncfile::SyncFile::from_native("");
        let wd = env::current_dir().unwrap();
        let mut testpath = PathBuf::from(wd);
        testpath.push("testdata");
        testpath.push("test_native_file.txt");
        //let (kw,relpath) = ("WHATEVER", "/testdata/test_native_file.txt""
        //let tf = format!("{}{})
        println!("{:?}", testpath);
        assert!(true);
    }
}
