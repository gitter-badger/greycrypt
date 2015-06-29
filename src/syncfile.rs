extern crate uuid;
extern crate crypto;
extern crate rand;
extern crate rustc_serialize;

use util;
use config;
use mapping;
use std::collections::HashMap;
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

const IVSize: usize = 16;

struct OpenFileState {
    handle: File,
    //iv: &'a [u8]
    iv: [u8;IVSize]
}
enum SyncFileState {
    Closed,
    Open(OpenFileState)
}
pub struct SyncFile {
    id: String,
    keyword: String,
    relpath: String,
    revguid: uuid::Uuid,
    nativefile: String,
    sync_file_state: SyncFileState
}

struct CryptoHelper {
    encryptor: Box<crypto::symmetriccipher::Encryptor>,
    got_eof_on_encrypt: bool,
    decryptor: Box<crypto::symmetriccipher::Decryptor>,
    got_eof_on_decrypt: bool
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
            decryptor: decryptor,
            got_eof_on_encrypt: false,
            got_eof_on_decrypt: false
        }
    }

    pub fn encrypt(&mut self, data: &[u8], is_all_data:bool) -> Result<Vec<u8>, symmetriccipher::SymmetricCipherError> {
        if self.got_eof_on_encrypt {
            panic!("Already received encryption eof, can't encrypt anymore; reinit crypto helper");
        }
        if is_all_data {
            self.got_eof_on_encrypt = true;
        }

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
        if self.got_eof_on_decrypt {
            panic!("Already received decryption eof, can't decrypt anymore; reinit crypto helper");
        }
        if is_all_data {
            self.got_eof_on_decrypt = true;
        }

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
            nativefile: nativefile.to_string(),
            sync_file_state: SyncFileState::Closed
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

        let mut ivline = String::new();
        let mut mdline = String::new();

        {
            let mut reader = BufReader::new(&fin);

            match reader.read_line(&mut ivline) {
                Err(e) => return Err(format!("Failed to read header line from syncfile: {:?}: {}", syncpath, e)),
                Ok(_) => ()
            }
            match reader.read_line(&mut mdline) {
                Err(e) => return Err(format!("Failed to read metadata line from syncfile: {:?}: {}", syncpath, e)),
                Ok(_) => ()
            }
        }

        let iv = ivline.from_base64().unwrap();// TODO check error
        if (iv.len() != IVSize) {
            return Err(format!("Unexpected IV length: {}", iv.len()));
        }

        // make crypto helper
        let key:&[u8] = &key;
        let iv:&[u8] = &iv;
        let mut crypto = CryptoHelper::new(key,iv);

        let md = mdline.from_base64();
        let md = match md {
            Err(e) => return Err(format!("Failed to unpack metadata: error {:?}, line: {:?}", e, md)),
            Ok(md) => md
        };

        let md = match crypto.decrypt(&md,true) {
            Err(e) => return Err(format!("Failed to decrypt meta data: {:?}", e)),
            Ok(md) => md
        };
        let md = String::from_utf8(md).unwrap();
        let md:Vec<&str> = md.lines().collect();

        // first line should be version, check that
        {
            let verline = md[0];
            let verparts:Vec<&str> = verline.split(":").collect();
            if verparts[0].trim() != "ver" {
                return Err(format!("expected first line of metadata to be file version, got: {:?}", verparts[0]));
            }
            if verparts[1].trim() != "1" { // lame
                return Err(format!("unexpected file version, got: {:?}", verline));
            }
        }

        // build md map and parse md - a little tedious, but lets us error out if any of the
        // values that we need are missing.
        let mdmap = {
            let mut mdmap:HashMap<String,String> = HashMap::new();
            for l in md {
                let parts:Vec<&str> = l.split(':').collect();
                println!("{:?}",parts);
                let k = parts[0].trim();
                let v = parts[1].trim();
                mdmap.insert(k.to_lowercase(),v.to_string());
            }
            mdmap
        };

        let keyword:String = {
            let v = mdmap.get("kw");
            match v {
                None => return Err(format!("Key 'kw' is required in metadata")),
                Some(v) => v.to_string()
            }
        };
        let relpath = {
            let v = mdmap.get("relpath");
            match v {
                None => return Err(format!("Key 'relpath' is required in metadata")),
                Some(v) => v.to_string()
            }
        };
        let revguid = {
            let v = mdmap.get("revguid");
            match v {
                None => return Err(format!("Key 'revguid' is required in metadata")),
                Some(v) => {
                    match uuid::Uuid::parse_str(v) {
                        Err(e) => return Err(format!("Failed to parse uuid: {}: {:?}", v, e)),
                        Ok(u) => u
                    }
                }
            }
        };

        // :(
        // http://stackoverflow.com/questions/29570607/is-there-a-good-way-to-convert-a-vect-to-an-array
        let mut iv_copy:[u8;IVSize] = [0;IVSize];
        for i in 0..IVSize {
            iv_copy[i] = iv[i]
        }
        let ofs = OpenFileState {
            handle: fin,
            iv: iv_copy
        };

        let idstr = SyncFile::get_sync_id(&keyword,&relpath);

        let mut sf = SyncFile {
            id: idstr,
            keyword: keyword,
            relpath: relpath,
            revguid: revguid,
            nativefile: "".to_string(),
            sync_file_state: SyncFileState::Open(ofs)
        };

        // try to set native file path; ignore failures for now, but we'll fail for real if
        // we try to write it.
        let _ = sf.set_nativefile_path(&conf);

        Ok(sf)
    }

    fn set_nativefile_path(&mut self, conf:&config::SyncConfig) -> Result<(),String> {
        // use the keyword to find the base path in the mapping, then join with the relpath
        let res = conf.mapping.lookup_dir(&self.keyword);
        match res {
            None => Err(format!("Keyword {} not found in mapping", &self.keyword)),
            Some(dir) => {
                let mut outpath = PathBuf::from(&dir);
                // pathbuf will mess up unless we chop leading path sep, and join ()
                // appears to do nothing...whatevs
                let rp = &util::decanon_path(&self.relpath[1..]);
                outpath.push(rp);
                self.nativefile = outpath.to_str().unwrap().to_string();
                Ok(())
            }
        }
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
        let mut crypto = CryptoHelper::new(key,iv);

        // write iv to file (unencrypted, base64 encoded)
        let _ = writeln!(fout, "{}", iv.to_base64(STANDARD));

        // write metadata (encrypted, base64 encoded string)
        let mut v:Vec<u8> = Vec::new();
        self.pack_header(&mut v);

        // pass true to signal EOF so that the metadata can be decrypted without needing to read
        // the whole file.
        let res = crypto.encrypt(&v[..], true);

        match res {
            Err(e) => panic!("Encryption error: {:?}", e),
            Ok(d) => {
                let b64_out = d[..].to_base64(STANDARD);
                let _ = writeln!(fout, "{}", b64_out);
            }
        }

        // remake crypto helper for file data
        let mut crypto = CryptoHelper::new(key,iv);

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
                    let res = crypto.encrypt(enc_bytes, eof);
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

        let savetp = testpath.to_str().unwrap();

        let conf = get_config();

        let sfpath = create_syncfile(&conf,&testpath);
        let sfpath = PathBuf::from(&sfpath);
        let res = syncfile::SyncFile::from_syncfile(&conf,&sfpath);
        match res {
            Err(e) => panic!("Error {:?}", e),
            Ok(sf) => {
                let eid = syncfile::SyncFile::get_sync_id(&sf.keyword,&sf.relpath);
                assert_eq!(eid,sf.id);
                assert_eq!(sf.keyword, "GCPROJROOT");
                assert_eq!(sf.relpath, "/testdata/test_native_file.txt");
                // revguid could be anything, but if it wasn't a guid we would already have failed
                assert_eq!(sf.nativefile, savetp);
                // file should be open
                match sf.sync_file_state {
                    syncfile::SyncFileState::Open(ofs) => {
                        // assume handle is valid (will check anyway when we read data)
                        // iv should be non-null
                        for x in 0..syncfile::IVSize {
                            assert!(ofs.iv[x] != 0);
                        }
                    },
                    _ => panic!("Unexpected file state")
                }
            }
        }
        //assert!(false);
    }
}
