extern crate uuid;
extern crate crypto;
extern crate rand;
extern crate rustc_serialize;

use util;
use config;
use mapping;
use std::path::{PathBuf};
use std::fs::File;
use std::io::Write;
use std::io::Read;
use std::result;
use std::boxed;
use self::crypto::digest::Digest;
use self::crypto::sha2::Sha256;

use self::crypto::{ symmetriccipher, buffer, aes, blockmodes };
use self::crypto::buffer::{ ReadBuffer, WriteBuffer, BufferResult };
use self::rand::{ Rng, OsRng };
use self::rustc_serialize::base64::{ToBase64, STANDARD};

pub struct SyncFile {
    id: String,
    keyword: String,
    relpath: String,
    revguid: uuid::Uuid,
    nativefile: String
}

struct EncryptHelper {
    encryptor: Box<crypto::symmetriccipher::Encryptor>
}

impl EncryptHelper {
    // don't get these arguments backwards ಠ_ಠ
    pub fn new(key:&[u8], iv:&[u8]) -> EncryptHelper {
        let encryptor = aes::cbc_encryptor(
                aes::KeySize::KeySize256,
                key,
                iv,
                blockmodes::PkcsPadding);
        EncryptHelper {
            encryptor: encryptor
        }
    }

    pub fn encrypt(&mut self, data: &[u8], is_all_data:bool) -> Result<Vec<u8>, symmetriccipher::SymmetricCipherError> {
        let mut final_result = Vec::<u8>::new();
        let mut read_buffer = buffer::RefReadBuffer::new(data);
        let mut buffer = [0; 4096];
        let mut write_buffer = buffer::RefWriteBuffer::new(&mut buffer);

        loop {
            let result = try!(self.encryptor.encrypt(&mut read_buffer, &mut write_buffer, is_all_data));

            final_result.extend(write_buffer.take_read_buffer().take_remaining().iter().map(|&i| i));

            match result {
                BufferResult::BufferUnderflow => break,
                BufferResult::BufferOverflow => { }
            }
        }

        Ok(final_result)
    }
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
            nativefile: nativefile.to_string()
        };

        Ok(ret)
    }

    pub fn read_data_and_save(self, conf:&config::SyncConfig) -> Result<(),String> {
        let mut outpath = PathBuf::from(&conf.sync_dir);
        // note set_file_name will wipe out the last part of the path, which is a directory
        // in this case. LOLOL
        outpath.push(self.id);
        outpath.set_extension("dat");
        println!("saving: {}",outpath.to_str().unwrap());
        let res = File::create(outpath.to_str().unwrap());

        let mut fout = match res {
            Err(e) => panic!("{:?}", e),
            Ok(f) => f
        };

        let key = match conf.encryption_key {
            None => panic!("No encryption key"),
            Some(k) => k
        };

        // create random iv
        let mut rng = OsRng::new().ok().unwrap();
        let mut iv: [u8; 16] = [0; 16];
        rng.fill_bytes(&mut iv);

        // make encryptor
        let key:&[u8] = &key;
        let iv:&[u8] = &iv;
        let mut encryptor = EncryptHelper::new(key,iv);

        // write iv to file (unencrypted, base64 encoded)
        let _ = writeln!(fout, "{}", iv.to_base64(STANDARD));

        // write metadata (encrypted, base64 encoded string)
        let mut v:Vec<u8> = Vec::new();
        let md_format_ver = 1;
        let _ = writeln!(v, "ver: {}", md_format_ver);
        let _ = writeln!(v, "kw: {}", self.keyword);
        let _ = writeln!(v, "relpath: {}", self.relpath);
        let _ = writeln!(v, "revguid: {}", self.revguid);

        let res = encryptor.encrypt(&v[..], false);
        match res {
            Err(e) => panic!("Encryption error: {:?}", e),
            Ok(d) => {
                let _ = writeln!(fout, "{}", d[..].to_base64(STANDARD));
            }
        }

        // read, encrypt, and write file data, not slurping because it could be big
        let mut fin = File::open(self.nativefile);
        match fin {
            Err(e) => { panic!("Can't open input native file: {}", e) },
            Ok(fin) => {
                let mut buf:[u8;65536] = [0; 65536];
                let mut fin = fin;

                loop {
                    let read_res = fin.read(&mut buf);
                    match read_res {
                        Err(e) => { panic!("Read error: {}", e) },
                        Ok(num_read) => {
                            let enc_bytes = &buf[0 .. num_read];
                            let eof = num_read == 0;
                            let res = encryptor.encrypt(enc_bytes, eof);
                            match res {
                                Err(e) => panic!("Encryption error: {:?}", e),
                                Ok(d) => {
                                    let _ = fout.write(&d); // TODO: check result
                                }
                            }
                            if eof {
                                let _ = fout.sync_all(); // TODO: use try!
                                break;
                            }
                        }
                    }
                }
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
                let mut outpath = PathBuf::from(&wd);
                outpath.push("testdata");
                outpath.push("syncdir");

                let ec: [u8;32] = [0; 32];

                let conf = config::SyncConfig {
                    sync_dir: outpath.to_str().unwrap().to_string(),
                    mapping: mapping,
                    encryption_key: Some(ec)
                };

                let mut sf = sf;
                let res = sf.read_data_and_save(&conf);
                assert_eq!(res,Ok(()));
                //assert!(false);
            }
        }
    }
}
