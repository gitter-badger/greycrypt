extern crate uuid;
extern crate crypto;
extern crate rand;
extern crate rustc_serialize;

use config;
use mapping;
use std::path::{PathBuf};
use std::fs::{File,create_dir_all};
use std::fs::{PathExt};
use std::io::{Read, Write, BufReader, BufRead};
use self::crypto::digest::Digest;
use self::crypto::sha2::Sha256;

use self::crypto::{ symmetriccipher, buffer, aes, blockmodes };
use self::crypto::buffer::{ ReadBuffer, WriteBuffer, BufferResult };
use self::rand::{ Rng, OsRng };
use self::rustc_serialize::base64::{ToBase64, STANDARD, FromBase64 };

pub struct SyncFile {
    id: String,
    keyword: String,
    relpath: String,
    revguid: uuid::Uuid,
    nativefile: String
}

struct CryptoHelper {
    encryptor: Box<crypto::symmetriccipher::Encryptor>,
    decryptor: Box<crypto::symmetriccipher::Decryptor>
}

impl CryptoHelper {
    // don't get these arguments backwards ಠ_ಠ
    pub fn new(key:&[u8], iv:&[u8]) -> CryptoHelper {
        let encryptor = aes::cbc_encryptor(
                aes::KeySize::KeySize256,
                key,
                iv,
                blockmodes::PkcsPadding);
        let mut decryptor = aes::cbc_decryptor(
                aes::KeySize::KeySize256,
                key,
                iv,
                blockmodes::PkcsPadding);
        CryptoHelper {
            encryptor: encryptor,
            decryptor: decryptor
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

    pub fn decrypt(&mut self, encrypted_data: &[u8], is_all_data:bool) -> Result<Vec<u8>, symmetriccipher::SymmetricCipherError> {
        let mut final_result = Vec::<u8>::new();
        let mut read_buffer = buffer::RefReadBuffer::new(encrypted_data);
        let mut buffer = [0; 4096];
        let mut write_buffer = buffer::RefWriteBuffer::new(&mut buffer);

        loop {
            let result = try!(self.decryptor.decrypt(&mut read_buffer, &mut write_buffer, is_all_data));
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

    pub fn from_syncfile(conf:&config::SyncConfig, syncpath:&PathBuf) -> Result<SyncFile,String> {
        if !syncpath.is_file() {
            return Err(format!("Syncfile does not exist: {:?}", syncpath));
        }
        let key = match conf.encryption_key {
            None => panic!("No encryption key"),
            Some(k) => k
        };

        // read first two lines
        // first line is IV, need it to initialize encryptor
        // use it to unpack/decrypt second line which is metadata
        // set fields from metadata
        // read file data from rest of file, decrypt, attach (ugh, may want stream it out, or save
        // that phase for a separate step)

        let mut fin = match File::open(syncpath.to_str().unwrap()) {
            Err(e) => return Err(format!("Can't open syncfile: {:?}: {}", syncpath, e)),
            Ok(fin) => fin
        };

        let mut reader = BufReader::new(fin);
        let mut ivline = String::new();
        match reader.read_line(&mut ivline) {
            Err(e) => return Err(format!("Failed to read header line from syncfile: {:?}: {}", syncpath, e)),
            Ok(_) => ()
        }
        let mut mdline = String::new();
        match reader.read_line(&mut mdline) {
            Err(e) => return Err(format!("Failed to read metadata line from syncfile: {:?}: {}", syncpath, e)),
            Ok(_) => ()
        }

        let iv = ivline.from_base64().unwrap();// TODO check error
        let iv:&[u8] = &iv;

        // make encryptor
        let key:&[u8] = &key;
        let iv:&[u8] = &iv;
        let mut crypto = CryptoHelper::new(key,iv);

        let md = mdline.from_base64();
        let md = match md {
            Err(e) => return Err(format!("Failed to unpack metadata: error {:?}, line: {:?}", e, md)),
            Ok(md) => md
        };

        let md = match crypto.decrypt(&md,false) {
            Err(e) => return Err(format!("Failed to decrypt meta data: {:?}", e)),
            Ok(md) => md
        };
        let md = String::from_utf8(md).unwrap();




        println!("{:?}",md);

        Ok(SyncFile {
            id: "".to_string(),
            keyword: "".to_string(),
            relpath: "".to_string(),
            revguid: uuid::Uuid::new_v4(),
            nativefile: "".to_string()
        })

    }

    fn pack_header(&self, v:&mut Vec<u8>) {
        let md_format_ver = 1;
        let _ = writeln!(v, "ver: {}", md_format_ver);
        let _ = writeln!(v, "kw: {}", self.keyword);
        let _ = writeln!(v, "relpath: {}", self.relpath);
        let _ = writeln!(v, "revguid: {}", self.revguid);
    }

    pub fn read_native_and_save(self, conf:&config::SyncConfig) -> Result<String,String> {
        let mut outpath = PathBuf::from(&conf.sync_dir);
        if !outpath.is_dir() {
            let res = create_dir_all(&outpath);
            match res {
                Err(e) => panic!("Failed to create output sync directory: {:?}: {:?}", outpath, e),
                Ok(_) => ()
            }
        }
        // note set_file_name will wipe out the last part of the path, which is a directory
        // in this case. LOLOL
        outpath.push(&self.id);
        outpath.set_extension("dat");

        let outname = outpath.to_str().unwrap();
        println!("saving: {}",outname);
        let res = File::create(outname);

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
        let mut encryptor = CryptoHelper::new(key,iv);

        // write iv to file (unencrypted, base64 encoded)
        let _ = writeln!(fout, "{}", iv.to_base64(STANDARD));

        // write metadata (encrypted, base64 encoded string)
        let mut v:Vec<u8> = Vec::new();
        self.pack_header(&mut v);

        let res = encryptor.encrypt(&v[..], false);
        match res {
            Err(e) => panic!("Encryption error: {:?}", e),
            Ok(d) => {
                let _ = writeln!(fout, "{}", d[..].to_base64(STANDARD));
            }
        }

        // read, encrypt, and write file data, not slurping because it could be big
        let mut fin = match File::open(self.nativefile) {
            Err(e) => { panic!("Can't open input native file: {}", e) },
            Ok(fin) => fin
        };

        let mut buf:[u8;65536] = [0; 65536];

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

        Ok(outname.to_string())
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

    fn get_config() -> config::SyncConfig {
        let wd = env::current_dir().unwrap();

        // generate a mock mapping, with keyword "gcprojroot" mapped to the project's root dir
        let wds = wd.to_str();
        let mapping = format!("gcprojroot = '{}'", wds.unwrap());
        let mapping = toml::Parser::new(&mapping).parse().unwrap();
        let mapping = mapping::Mapping::new(&mapping).ok().expect("WTF?");

        let mut outpath = PathBuf::from(&wd);
        outpath.push("testdata");
        outpath.push("syncdir");

        let ec: [u8;32] = [0; 32];

        let conf = config::SyncConfig {
            sync_dir: outpath.to_str().unwrap().to_string(),
            mapping: mapping,
            encryption_key: Some(ec)
        };
        conf
    }

    fn create_syncfile(conf:&config::SyncConfig, testpath:&PathBuf) -> String {
        let res = syncfile::SyncFile::from_native(&conf.mapping, testpath.to_str().unwrap());
        match res {
            Err(m) => panic!(m),
            Ok(sf) => {
                let res = sf.read_native_and_save(&conf);
                match res {
                    Err(e) => panic!("{}", e),
                    Ok(sfpath) => sfpath
                }
            }
        }
    }

    #[test]
    fn test_write_read_syncfile() {
        let wd = env::current_dir().unwrap();
        let mut testpath = PathBuf::from(&wd);
        testpath.push("testdata");
        testpath.push("test_native_file.txt");

        let conf = get_config();

        let sfpath = create_syncfile(&conf,&testpath);
        let sfpath = PathBuf::from(&sfpath);
        let res = syncfile::SyncFile::from_syncfile(&conf,&sfpath);
        match res {
            Err(e) => panic!("Error {:?}", e),
            Ok(sf) => {}
        }
        assert!(false);
    }
}
