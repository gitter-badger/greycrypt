extern crate uuid;
extern crate crypto;

use util;
use config;
use mapping;
use std::path::{PathBuf};
use std::fs::File;
use std::io::Write;
use std::result;
use self::crypto::digest::Digest;
use self::crypto::sha2::Sha256;

pub struct SyncFile {
    id: String,
    keyword: String,
    relpath: String,
    revguid: uuid::Uuid,
    nativefile: String,
    data: Option<Vec<u8>>
}

impl SyncFile {
    pub fn get_sync_id(kw: &str, relpath: &str) -> String {
        // make id from hash of kw + relpath
        let mut hasher = Sha256::new();
        hasher.input_str(kw);
        hasher.input_str(&relpath.to_uppercase());
        hasher.result_str()
    }

    pub fn from_native(mapping: &mapping::Mapping, nativefile: &str) -> Result<SyncFile,String> {
        let (kw,relpath) = {
            let res = mapping.get_kw_relpath(nativefile);
            match res {
                None => return Err(format!("No mapping found for native file: {}", nativefile)),
                Some((kw,relpath)) => (kw,relpath)
            }
        };

        let idstr = SyncFile::get_sync_id(kw,&relpath);
        let ret = SyncFile {
            id: idstr,
            keyword: kw.to_string(),
            relpath: relpath,
            revguid: uuid::Uuid::new_v4(),
            nativefile: nativefile.to_string(),
            data: None
        };

        Ok(ret)
    }

    pub fn attach_data(&mut self) {
        let data = util::slurp_bin_file(&self.nativefile);
        // todo! encrypt
        self.data = Some(data);
    }

    pub fn save(self, conf:&config::SyncConfig) -> Result<(),String> {
        let mut outpath = PathBuf::from(&conf.sync_dir);
        // note set_file_name will wipe out the last part of the path, which is a directory
        // in this case. LOLOL
        outpath.push(self.id);
        outpath.set_extension("dat");
        println!("saving: {}",outpath.to_str().unwrap());
        let res = File::create(outpath.to_str().unwrap());
        let mut f = match res {
            Err(e) => panic!("{:?}", e),
            Ok(f) => f
        };
        let data = self.data.unwrap();
        let res = f.write_all(&data);
        match res {
            Err(e) => panic!("{:?}", e),
            Ok(_) => {
                let _ = f.sync_all(); // TODO: use try!
                ()
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::path::{PathBuf};
    use config;
    use mapping;
    use syncfile;

    extern crate toml;

    #[test]
    fn create_syncfile() {
        let wd = env::current_dir().unwrap();

        // generate a mock mapping
        let wds = wd.to_str();
        let mapping = format!("gcprojroot = '{}'", wds.unwrap());
        let mapping = toml::Parser::new(&mapping).parse().unwrap();
        let mapping = mapping::Mapping::new(&mapping).ok().expect("WTF?");

        let mut testpath = PathBuf::from(&wd);
        testpath.push("testdata");
        testpath.push("test_native_file.txt");

        let res = syncfile::SyncFile::from_native(&mapping, testpath.to_str().unwrap());
        match res {
            Err(m) => panic!(m),
            Ok(sf) => {
                let mut sf = sf;
                sf.attach_data();

                let mut outpath = PathBuf::from(&wd);
                outpath.push("testdata");
                outpath.push("syncdir");

                let conf = config::SyncConfig {
                    sync_dir: outpath.to_str().unwrap().to_string(),
                    mapping: mapping
                };

                let res = sf.save(&conf);
                assert_eq!(res,Ok(()));
                //assert!(false);
            }
        }
    }
}
