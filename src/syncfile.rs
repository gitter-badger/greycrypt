extern crate uuid;
extern crate crypto;
extern crate rand;
extern crate rustc_serialize;

use util;
use config;
use crypto_util;

use std::str::FromStr;
use std::collections::HashMap;
use std::path::{PathBuf};
use std::fs::{File,create_dir_all};
use std::fs::{PathExt};
use std::io::{Read, Write, BufReader, BufRead, SeekFrom, Seek,Result};
use std::io;
use util::make_err;

use self::crypto::digest::Digest;
use self::crypto::sha2::Sha256;

use self::rand::{ Rng, OsRng };
use self::rustc_serialize::base64::{ToBase64, STANDARD, FromBase64 };

const IV_SIZE: usize = 16;

struct OpenFileState {
    handle: File,
    iv: [u8;IV_SIZE]
}
enum SyncFileState {
    Closed,
    Open(OpenFileState)
}
pub struct SyncFile {
    pub id: String,
    pub keyword: String,
    pub relpath: String,
    pub revguid: uuid::Uuid,
    pub nativefile: String,
    pub is_binary: bool,
    pub is_deleted: bool,
    sync_file_state: SyncFileState
}

impl SyncFile {
    pub fn get_sync_id(kw: &str, relpath: &str) -> String {
        // make id from hash of kw + relpath
        let mut hasher = Sha256::new();
        hasher.input_str(kw);
        hasher.input_str(&relpath.to_uppercase());
        hasher.result_str()
    }

    // Return the sync id and syncfile path for a given native file.  Note, the path in
    // particular is a "default" setting, in a real sync scenario, it may be renamed based
    // on network sync state.  This is handled by the option parameter to create_syncfile()
    // below.
    pub fn get_sync_id_and_path(conf:&config::SyncConfig, nativefile: &str) -> Result<(String,PathBuf)> {
        let (kw,relpath) = {
            let res = conf.mapping.get_kw_relpath(nativefile);
            match res {
                None => return make_err(&format!("No mapping found for native file: {}", nativefile)),
                Some((kw,relpath)) => (kw,relpath)
            }
        };
        let idstr = SyncFile::get_sync_id(kw,&relpath);

        let mut syncpath = PathBuf::from(&conf.sync_dir());
        let prefix = &idstr.to_owned()[0..2];
        syncpath.push(prefix);
        syncpath.push(&idstr);
        syncpath.set_extension("dat");

        Ok((idstr,syncpath))
    }

    pub fn set_deleted(&mut self) {
        *self = SyncFile {
            id: self.id.clone(),
            keyword: self.keyword.clone(),
            relpath: self.relpath.clone(),
            revguid: uuid::Uuid::new_v4(),
            nativefile: self.nativefile.to_owned(),
            is_binary: self.is_binary,
            is_deleted: true,
            sync_file_state: SyncFileState::Closed
        };
    }

    pub fn from_native(conf:&config::SyncConfig, nativefile: &str) -> Result<SyncFile> {
        let (kw,relpath) = {
            let res = conf.mapping.get_kw_relpath(nativefile);
            match res {
                None => return make_err(&format!("No mapping found for native file: {}", nativefile)),
                Some((kw,relpath)) => (kw,relpath)
            }
        };

        let idstr = SyncFile::get_sync_id(kw,&relpath);

        let is_binary = match util::file_is_binary(nativefile) {
            Err(e) => return make_err(&format!("Failed to check binary status: {:?}", e)),
            Ok(isb) => isb
        };

        let ret = SyncFile {
            id: idstr,
            keyword: kw.to_owned(),
            relpath: relpath,
            revguid: uuid::Uuid::new_v4(),
            nativefile: nativefile.to_owned(),
            is_binary: is_binary,
            is_deleted: false,
            sync_file_state: SyncFileState::Closed
        };

        Ok(ret)
    }

    fn read_top_lines(fin:&File,count:i32) -> Result<Vec<String>> {
        let mut reader = BufReader::new(fin);

        let mut lines:Vec<String> = Vec::new();
        for i in 0 .. count {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Err(e) => return make_err(&format!("Failed to read header line {} from syncfile: {}", i, e)),
                Ok(_) => {
                    lines.push(line.trim().to_owned());
                }
            }
        }

        // This seek is weird, but if we don't do it, we get a zero
        // byte read when we try to read the file data after the header lines.
        // Apparently this has the effect of repositioning the file at the unbuffered cursor
        // position even though the buffered reader has already read a block of data.
        // https://github.com/rust-lang/rust/blob/9cc0b2247509d61d6a246a5c5ad67f84b9a2d8b6/src/libstd/io/buffered.rs#L305
        let res = reader.seek(SeekFrom::Current(0));
        match res {
            Err(e) => return make_err(&format!("Failed to seek reader after metadata: {:?}", e)),
            Ok(_) => ()
        }

        Ok(lines)
    }

    pub fn get_syncid_from_file(syncpath:&PathBuf) -> Result<String> {
        if !syncpath.is_file() {
            return make_err(&format!("Syncfile does not exist: {:?}", syncpath));
        }
        let fin = match File::open(syncpath.to_str().unwrap()) {
            Err(e) => return make_err(&format!("Can't open syncfile: {:?}: {}", syncpath, e)),
            Ok(fin) => fin
        };
        match SyncFile::read_top_lines(&fin,1) {
            Err(e) => return Err(e),
            Ok(lines) => {
                Ok(lines[0].to_owned())
            }
        }
    }

    fn init_sync_read(conf:&config::SyncConfig, syncpath:&PathBuf) -> Result<(File,String,[u8;IV_SIZE],HashMap<String,String>)> {
        let key = match conf.encryption_key {
            None => return make_err(&"No encryption key".to_owned()),
            Some(k) => k
        };

        if !syncpath.is_file() {
            return make_err(&format!("Syncfile does not exist: {:?}", syncpath));
        }

        // read first n lines:
        // syncid
        // iv
        // metadata
        // use IV, to initialize crypto helper
        // use base64/helper to unpack/decrypt second line which is metadata
        // set fields from metadata
        // leave file handle open for later decryption of content data

        let fin = match File::open(syncpath.to_str().unwrap()) {
            Err(e) => return make_err(&format!("Can't open syncfile: {:?}: {}", syncpath, e)),
            Ok(fin) => fin
        };

        let (syncid,ivline,mdline) = {
            match SyncFile::read_top_lines(&fin,3) {
                Err(e) => return Err(e),
                Ok(lines) => {
                    // if any are empty, its an error
                    for l in &lines {
                        if l.trim() == "" {
                            return make_err(&format!("Found empty line in syncfile, file is invalid: {:?}; may need to be removed", syncpath))
                        }
                    }
                    (lines[0].clone(), lines[1].clone(), lines[2].clone())
                }
            }
        };

        let iv = match ivline.from_base64() {
            Err(e) => return make_err(&format!("Unable to parse IV line: {}", e)),
            Ok(iv) => iv
        };
        if iv.len() != IV_SIZE {
            return make_err(&format!("Unexpected IV length: {}", iv.len()));
        }

        // make crypto helper
        let key:&[u8] = &key;
        let iv:&[u8] = &iv;
        let mut crypto = crypto_util::CryptoHelper::new(key,iv);

        let md = mdline.from_base64();
        let md = match md {
            Err(e) => return make_err(&format!("Failed to unpack metadata: error {:?}, line: {:?}", e, md)),
            Ok(md) => md
        };

        let md = match crypto.decrypt(&md,true) {
            Err(e) => return make_err(&format!("Failed to decrypt meta data: {:?}; are you using the correct password?", e)),
            Ok(md) => md
        };
        let md = String::from_utf8(md).unwrap();
        let md:Vec<&str> = md.lines().collect();

        // first line should be version, check that
        {
            let verline = md[0];
            let verparts:Vec<&str> = verline.split(":").collect();
            if verparts[0].trim() != "ver" {
                return make_err(&format!("expected first line of metadata to be file version, got: {:?}", verparts[0]));
            }
            if verparts[1].trim() != "1" { // lame
                return make_err(&format!("unexpected file version, got: {:?}", verline));
            }
        }

        // build md map and parse md - a little tedious, but lets us error out if any of the
        // values that we need are missing.
        let mdmap = {
            let mut mdmap:HashMap<String,String> = HashMap::new();
            for l in md {
                let parts:Vec<&str> = l.split(':').collect();
                let k = parts[0].trim();
                let v = parts[1].trim();
                mdmap.insert(k.to_lowercase(),v.to_owned());
            }
            mdmap
        };

        // :(
        // http://stackoverflow.com/questions/29570607/is-there-a-good-way-to-convert-a-vect-to-an-array
        let mut iv_copy:[u8;IV_SIZE] = [0;IV_SIZE];
        for i in 0..IV_SIZE {
            iv_copy[i] = iv[i]
        }

        Ok((fin,syncid.to_owned(),iv_copy,mdmap))
    }

    pub fn get_metadata_hash(conf:&config::SyncConfig, syncpath:&PathBuf) -> Result<HashMap<String,String>> {
        let (_,_,_,mdmap) = match SyncFile::init_sync_read(conf,syncpath) {
            Err(e) => return Err(e),
            Ok(stuff) => stuff
        };
        Ok(mdmap)
    }

    pub fn from_syncfile(conf:&config::SyncConfig, syncpath:&PathBuf) -> Result<SyncFile> {
        let (fin,_,iv,mdmap) = match SyncFile::init_sync_read(conf,syncpath) {
            Err(e) => return Err(e),
            Ok(stuff) => stuff
        };
        
        let keyword:String = {
            match mdmap.get("kw") {
                None => return make_err(&format!("Key 'kw' is required in metadata")),
                Some(v) => v.to_owned()
            }
        };
        let relpath = {
            match mdmap.get("relpath") {
                None => return make_err(&format!("Key 'relpath' is required in metadata")),
                Some(v) => v.to_owned()
            }
        };
        let revguid = {
            match mdmap.get("revguid") {
                None => return make_err(&format!("Key 'revguid' is required in metadata")),
                Some(v) => {
                    match uuid::Uuid::parse_str(v) {
                        Err(e) => return make_err(&format!("Failed to parse uuid: {}: {:?}", v, e)),
                        Ok(u) => u
                    }
                }
            }
        };
        let is_binary = {
            match mdmap.get("is_binary") {
                None => return make_err(&format!("Key 'is_binary' is required in metadata")),
                Some(v) => {
                    match bool::from_str(v) {
                        Err(e) => return make_err(&format!("Failed to parse is_binary bool: {}", e)),
                        Ok(b) => b
                    }
                }
            }
        };
        let is_deleted = {
            match mdmap.get("is_deleted") {
                // if it ain't there it ain't deleted
                None => false,
                Some(v) => {
                    match bool::from_str(v) {
                        Err(e) => return make_err(&format!("Failed to parse is_deleted bool: {}", e)),
                        Ok(b) => b
                    }
                }
            }
        };

        // :(
        // http://stackoverflow.com/questions/29570607/is-there-a-good-way-to-convert-a-vect-to-an-array
        let mut iv_copy:[u8;IV_SIZE] = [0;IV_SIZE];       
        for i in 0..IV_SIZE {
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
            nativefile: "".to_owned(),
            is_binary: is_binary,
            is_deleted: is_deleted,
            sync_file_state: SyncFileState::Open(ofs)
        };

        // try to set native file path; ignore failures for now, but we'll fail for real if
        // we try to write it.
        let _ = sf.set_nativefile_path(&conf);

        Ok(sf)
    }

    fn set_nativefile_path(&mut self, conf:&config::SyncConfig) -> Result<()> {
        // use the keyword to find the base path in the mapping, then join with the relpath
        let res = conf.mapping.lookup_dir(&self.keyword);
        match res {
            None => make_err(&format!("Keyword {} not found in mapping", &self.keyword)),
            Some(dir) => {
                let mut outpath = PathBuf::from(&dir);
                // pathbuf will mess up unless we chop leading path sep, and join ()
                // appears to do nothing...whatevs
                let rp = &util::decanon_path(&self.relpath[1..]);
                outpath.push(rp);
                self.nativefile = outpath.to_str().unwrap().to_owned();
                Ok(())
            }
        }
    }

    fn pack_metadata(&self, conf:&config::SyncConfig, v:&mut Vec<u8>) -> io::Result<()> {
        let md_format_ver = 1;
        try!(writeln!(v, "ver: {}", md_format_ver));
        try!(writeln!(v, "kw: {}", self.keyword));
        try!(writeln!(v, "relpath: {}", self.relpath));
        try!(writeln!(v, "revguid: {}", self.revguid));
        try!(writeln!(v, "is_binary: {}", self.is_binary));
        try!(writeln!(v, "is_deleted: {}", self.is_deleted));

        // additional fields that aren't required for sync but are helpful for resolving conflicts
        let mtime = {
            if !self.is_deleted {
                match util::get_file_mtime(&self.nativefile) {
                    Err(e) => return Err(io::Error::new(io::ErrorKind::Other, format!("Failed to obtain mtime: {}",e))),
                    Ok(mtime) => mtime
                }
            } else {
                0
            }
        };
        try!(writeln!(v, "origin_native_mtime: {}", mtime));
        try!(writeln!(v, "origin_host: {}", conf.host_name));
        
        Ok(())
    }

    fn decrypt_helper(&mut self, conf:&config::SyncConfig, out:&mut Write) -> Result<()> {
        {
            let ofs = {
                match self.sync_file_state {
                    SyncFileState::Open(ref ofs) => ofs,
                    _ => return make_err(&"Sync file not open".to_owned())
                }
            };
            let key = match conf.encryption_key {
                None => return make_err(&"No encryption key".to_owned()),
                Some(k) => k
            };
            // make crypto helper
            let key:&[u8] = &key;
            let iv:&[u8] = &ofs.iv;
            let mut crypto = crypto_util::CryptoHelper::new(key,iv);

            let mut fin = &ofs.handle;

            let mut buf:[u8;65536] = [0; 65536];

            loop {
                let read_res = fin.read(&mut buf);
                match read_res {
                    Err(e) => { return make_err(&format!("Read error: {}", e)) },
                    Ok(num_read) => {
                        let enc_bytes = &buf[0 .. num_read];
                        let eof = num_read == 0;
                        let res = crypto.decrypt(enc_bytes, eof);
                        match res {
                            Err(e) => return make_err(&format!("Encryption error: {:?}", e)),
                            Ok(d) => {
                                let dlen = d.len();
                                try!(out.write_all(&d))
                            }
                        }
                        if eof {
                            match out.flush() {
                                Err(e) => return make_err(&format!("Failed to flush output reader: {}",e)),
                                Ok(_) => ()
                            }
                            break;
                        }
                    }
                }
            }
        }

        // close input file now
        self.close();

        Ok(())
    }
    
    pub fn close(&mut self) {
        self.sync_file_state = SyncFileState::Closed;
    }

    pub fn decrypt_to_writer(&mut self, conf:&config::SyncConfig, out:&mut Write) -> Result<()> {
        // if file is binary, can go directly to target_out.  otherwise, have to
        // stream to intermediate buffer and nativize the line endings.

        if self.is_binary {
            self.decrypt_helper(conf,out)
        } else {
            let mut temp_out:Vec<u8> = Vec::new();
            
            try!(self.decrypt_helper(conf,&mut temp_out));
            
            //println!("dec: {:?}", String::from_utf8(temp_out.clone()).unwrap());
            match util::decanon_lines(&temp_out) {
                Err(e) => return make_err(&format!("Decanon error: {}", e)),
                Ok(temp_out) => {
                    try!(out.write_all(&temp_out)); 
                }
            }

            Ok(())
        }
    }

    pub fn restore_native(&mut self, conf:&config::SyncConfig) -> Result<String> {
        { // check to make sure file is open, scoped to prevent borrow conflicts
            match self.sync_file_state {
                SyncFileState::Open(ref ofs) => ofs,
                _ => return make_err(&"Sync file not open".to_owned())
            };
        }

        let nativefile = self.nativefile.clone();
        let outpath = match nativefile.trim() {
            "" => return make_err(&"Native path not set, call set_nativefile_path()".to_owned()),
            s => s
        };

        let outpath_par = PathBuf::from(&outpath);
        let outpath_par = outpath_par.parent().unwrap();
        if !outpath_par.is_dir() {
            let res = create_dir_all(&outpath_par);
            match res {
                Err(e) => return make_err(&format!("Failed to create output local directory: {:?}: {:?}", outpath_par, e)),
                Ok(_) => ()
            }
        }

        // prep output handles
        let res = File::create(outpath);
        let mut fout = match res {
            Err(e) => return make_err(&format!("Failed to create output file: {:?}: {:?}", outpath, e)),
            Ok(f) => f
        };

        match self.decrypt_to_writer(conf,&mut fout) {
            Err(e) => return make_err(&format!("Failed to decrypt file: {:?}: {:?}", outpath, e)),
            Ok(_) => ()
        }

        Ok(outpath.to_owned())
    }

    fn write_syncfile_header(&self, conf:&config::SyncConfig, override_path: Option<PathBuf>) ->
    // Ret (outpath,outhandle,iv,key)
    Result<(String,File,[u8;IV_SIZE],[u8;config::KEY_SIZE])> {
        let (sid,outpath) = match SyncFile::get_sync_id_and_path(conf,&self.nativefile) {
            Err(e) => return make_err(&format!("Can't get id/path: {:?}", e)),
            Ok(pair) => pair
        };

        let outpath = match override_path {
            None => outpath,
            Some(path) => path
        };

        let outpath_par = outpath.parent().unwrap();
        if !outpath_par.is_dir() {
            let res = create_dir_all(&outpath_par);
            match res {
                Err(e) => return make_err(&format!("Failed to create output sync directory: {:?}: {:?}", outpath_par, e)),
                Ok(_) => ()
            }
        }

        let outname = outpath.to_str().unwrap();
        let mut fout = match File::create(outname) {
            Err(e) => return make_err(&format!("Can't create output file: {:?}", e)),
            Ok(f) => f
        };

        let key = match conf.encryption_key {
            None => return make_err(&format!("No encryption key")),
            Some(k) => k
        };

        // create random iv
        let mut rng = OsRng::new().ok().unwrap();
        let mut iv: [u8; IV_SIZE] = [0; IV_SIZE];
        rng.fill_bytes(&mut iv);

        // make crypto helper
        let mut crypto = crypto_util::CryptoHelper::new(&key,&iv);

        // write sync id to file (unencrypted)
        try!(writeln!(fout, "{}", sid));
        // write iv to file (unencrypted, base64 encoded)
        try!(writeln!(fout, "{}", iv.to_base64(STANDARD)));
        // write metadata (encrypted, base64 encoded string)
        let mut v:Vec<u8> = Vec::new();
        try!(self.pack_metadata(conf, &mut v));
        // pass true to indicate EOF so that the metadata can be decrypted without needing to read
        // the whole file.
        let res = crypto.encrypt(&v[..], true);

        match res {
            Err(e) => return make_err(&format!("Encryption error: {:?}", e)),
            Ok(d) => {
                let b64_out = d[..].to_base64(STANDARD);
                match writeln!(fout, "{}", b64_out) {
                    Err(e) => return make_err(&format!("Failed to write metadata: {}", e)),
                    Ok(_) => ()
                }
            }
        }

        Ok((outname.to_owned(),fout,iv,key))
    }

    // TODO: should make this pure, return a new syncfile
    pub fn mark_deleted_and_save(&mut self, conf:&config::SyncConfig, override_path: Option<PathBuf>) -> Result<String> {
        self.set_deleted();
        let (outname,_,_,_) = match self.write_syncfile_header(conf,override_path) {
            Err(e) => return make_err(&format!("Failed to write syncfile header: {}", e)),
            Ok(stuff) => stuff
        };
        Ok(outname)
    }

    pub fn read_native_and_save(&self, conf:&config::SyncConfig, override_path: Option<PathBuf>) -> Result<String> {
        let (outname,fout,iv,key) = match self.write_syncfile_header(conf,override_path) {
            Err(e) => return make_err(&format!("Failed to write syncfile header: {}", e)),
            Ok(stuff) => stuff
        };
        let mut fout = fout;

        // remake crypto helper for file data
        let mut crypto = crypto_util::CryptoHelper::new(&key,&iv);

        // read, encrypt, and write file data, not slurping because it could be big
        let mut fin = match File::open(&self.nativefile) {
            Err(e) => return make_err(&format!("Can't open input native file: {}: {}", &self.nativefile, e)),
            Ok(fin) => fin
        };

        if self.is_binary {
            // stream-encrypt binary files
            
            // use vec to heap alloc the buffer
            const SIZE: usize = 1048576;
            let mut v: Vec<u8> = vec![0;SIZE];
            let mut buf = &mut v;

            loop {
                let num_read = try!(fin.read(buf));
                let enc_bytes = &buf[0 .. num_read];
                let eof = num_read == 0;
                let res = crypto.encrypt(enc_bytes, eof);
                match res {
                    Err(e) => return make_err(&format!("Encryption error: {:?}", e)),
                    Ok(d) => {
                        let dlen = d.len();
                        try!(fout.write_all(&d))
                    }
                }
                //println!("encrypted {} bytes",num_read);
                if eof {
                    break;
                }
            }
        } else {
            // for text files, read them in and normalized the line endings (use \n), so that
            // the (decrypted) binary value is same on all platforms.  this is required for de-dup
            // comparisons.  when unpacking to native on a target platform, we'll restore the
            // proper line endings
            let br = BufReader::new(fin);
            let in_lines = br.lines();
            let mut out_lines:Vec<String> = Vec::new();
            for l in in_lines {
                match l {
                    Err(e) => return make_err(&format!("Failed to read line from alleged text source: {}", e)),
                    Ok(l) => {
                        out_lines.push(l.to_owned());
                    }
                }
            }

            let line_buf = match util::canon_lines(&out_lines) {
                Err(e) => return make_err(&format!("{}", e)),
                Ok(buf) => buf
            };

            let enc_bytes = &line_buf[0 .. line_buf.len()];

            match crypto.encrypt(enc_bytes, true) {
                Err(e) => return make_err(&format!("Encryption error: {:?}", e)),
                Ok(d) => {
                    let dlen = d.len();
                    try!(fout.write_all(&d))
                }
            }
        }

        Ok(outname.to_owned())
    }

    pub fn create_syncfile(conf:&config::SyncConfig, nativepath:&PathBuf, override_path: Option<PathBuf>) -> Result<(String,SyncFile)> {
        let res = SyncFile::from_native(&conf, nativepath.to_str().unwrap());
        let sf = match res {
            Err(e) => return make_err(&format!("Failed to create sync file: {:?}", e)),
            Ok(sf) => sf
        };

        let res = sf.read_native_and_save(&conf, override_path);
        match res {
            Err(e) => return make_err(&format!("Failed to update sync file with native data: {:?}", e)),
            Ok(sfpath) => Ok((sfpath,sf))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::env;
    // use std::fs::File;
    // use std::io::Write;
    use std::path::{PathBuf};
    use util;
    use mapping;
    use syncfile;
    use testlib;

    extern crate toml;

    #[test]
    fn write_read_syncfile() {
        let wd = env::current_dir().unwrap();
        let mut testpath = PathBuf::from(&wd);
        testpath.push("testdata");
        testpath.push("test_text_file.txt");

        let savetp = testpath.to_str().unwrap();

        let mut conf = testlib::util::get_mock_config();

        let sfpath = match syncfile::SyncFile::create_syncfile(&conf,&testpath,None) {
            Err(e) => panic!("Error {:?}", e),
            Ok((sfpath,_)) => sfpath
        };
        let sfpath = PathBuf::from(&sfpath);

        let file_syncid = match syncfile::SyncFile::get_syncid_from_file(&sfpath) {
            Err(e) => panic!("Error {:?}", e),
            Ok(id) => id
        };

        let res = syncfile::SyncFile::from_syncfile(&conf,&sfpath);
        match res {
            Err(e) => panic!("Error {:?}", e),
            Ok(sf) => {
                let eid = syncfile::SyncFile::get_sync_id(&sf.keyword,&sf.relpath);
                assert_eq!(eid,sf.id);
                assert_eq!(eid,file_syncid);
                assert_eq!(sf.keyword, "GCPROJROOT");
                assert_eq!(sf.relpath, "/testdata/test_text_file.txt");
                // revguid could be anything, but if it wasn't a guid we would already have failed
                assert_eq!(sf.nativefile, savetp);
                assert_eq!(sf.is_binary, false);
                assert_eq!(sf.is_deleted, false);
                // file should be open
                if let syncfile::SyncFileState::Open(ref ofs) = sf.sync_file_state {
                        // assume handle is valid (will check anyway when we read data)
                        // iv should be non-null (though its possible that it could
                        // be randomly all zeros, that should be very rare)
                        let mut zcount = 0;
                        for x in 0..syncfile::IV_SIZE {
                            if ofs.iv[x] == 0 { zcount = zcount + 1 }
                        }
                        assert!(zcount != syncfile::IV_SIZE)                
                } else {
                    panic!("Unexpected file state")
                }

                // remap the keyword var to the "nativedir" under testdata
                let wds = wd.to_str().unwrap();
                let mut outpath = PathBuf::from(&wds);
                outpath.push("testdata");
                outpath.push("out_nativedir");

                let mapping = format!("gcprojroot = '{}'", outpath.to_str().unwrap());
                let mapping = toml::Parser::new(&mapping).parse().unwrap();
                let mapping = mapping::Mapping::new(&mapping).ok().expect("WTF?");
                conf.mapping = mapping;

                // reset native path
                let mut sf = sf;
                assert!(sf.set_nativefile_path(&conf).is_ok());
                let res = sf.restore_native(&conf);
                let outfile = {
                    match res {
                        Err(e) => panic!("Error {:?}", e),
                        Ok(outfile) => {
                            let mut ex_out = outpath.clone();
                            ex_out.push("testdata");
                            ex_out.push("test_text_file.txt");
                            assert_eq!(outfile, ex_out.to_str().unwrap());
                            outfile
                        }
                    }
                };

                // slurp source and output files and compare
                let srctext = util::slurp_text_file(&savetp.to_owned());
                let outtext = util::slurp_text_file(&outfile);
                assert_eq!(srctext,outtext);
            }
        }
    }

    #[test]
    fn write_read_binary_file() {
        let wd = env::current_dir().unwrap();
        let mut testpath = PathBuf::from(&wd);
        testpath.push("testdata");
        testpath.push("test_binary.png");

        let in_bytes = util::slurp_bin_file(testpath.to_str().unwrap());

        let mut conf = testlib::util::get_mock_config();

        let sfpath = match syncfile::SyncFile::create_syncfile(&conf,&testpath,None) {
            Err(e) => panic!("Error {:?}", e),
            Ok((sfpath,sf)) => {
                assert!(sf.is_binary);
                sfpath
            }
        };
        let sfpath = PathBuf::from(&sfpath);

        let res = syncfile::SyncFile::from_syncfile(&conf,&sfpath);
        match res {
            Err(e) => panic!("Error {:?}", e),
            Ok(sf) => {
                assert!(sf.is_binary);

                // remap the keyword var to the "nativedir" under testdata
                let wds = wd.to_str().unwrap();
                let mut outpath = PathBuf::from(&wds);
                outpath.push("testdata");
                outpath.push("out_nativedir");

                let mapping = format!("gcprojroot = '{}'", outpath.to_str().unwrap());
                let mapping = toml::Parser::new(&mapping).parse().unwrap();
                let mapping = mapping::Mapping::new(&mapping).ok().expect("WTF?");
                conf.mapping = mapping;

                // reset native path
                let mut sf = sf;

                assert!(sf.set_nativefile_path(&conf).is_ok());
                let res = sf.restore_native(&conf);
                let outfile = {
                    match res {
                        Err(e) => panic!("Error {:?}", e),
                        Ok(outfile) => outfile
                    }
                };

                let out_bytes = util::slurp_bin_file(&outfile);
                assert_eq!(in_bytes,out_bytes);
            }
        }
    }

    #[test]
    fn decrypt_to_mem() {
        let conf = testlib::util::get_mock_config();

        let wd = env::current_dir().unwrap();
        let mut testpath = PathBuf::from(&wd);
        testpath.push("testdata");
        testpath.push("test_text_file.txt");
        let srctext = util::slurp_bin_file(&testpath.to_str().unwrap().to_owned());

        let mut syncpath = PathBuf::from(&wd);
        syncpath.push("testdata");
        syncpath.push("6539709be17615dbbf5d55f84f293c55ecc50abf4865374c916bef052e713fec.dat");

        let mut sf = match syncfile::SyncFile::from_syncfile(&conf,&syncpath) {
            Err(e) => panic!("Failed to read syncfile: {:?}", e),
            Ok(sf) => sf
        };

        let mut data:Vec<u8> = Vec::new();

        match sf.decrypt_to_writer(&conf, &mut data) {
            Err(e) => panic!("Error {:?}", e),
            Ok(_) => {
                //println!("srclen: {}; datalen: {}", srctext.len(), data.len());

                // uncomment to see what the data looks like in case this fails
                // match File::create("testdata/temp.out") {
                //     Err(e) => panic!("Error {:?}", e),
                //     Ok(ref mut f) => { f.write_all(&data); () }
                // }
                assert_eq!(srctext,data);


            }
        }
    }

    #[test]
    fn deleted() {
        let conf = testlib::util::get_mock_config();
        let wd = env::current_dir().unwrap();

        let readit = |path| {
            let sf = match syncfile::SyncFile::from_syncfile(&conf,&path) {
                Err(e) => panic!("Failed to read syncfile: {:?}", e),
                Ok(sf) => sf
            };
            sf
        };

        let mut sf = {
            let mut syncpath = PathBuf::from(&wd);
            syncpath.push("testdata");
            syncpath.push("6539709be17615dbbf5d55f84f293c55ecc50abf4865374c916bef052e713fec.dat");

            readit(syncpath)
        };
        let start_revguid = sf.revguid;

        let mut syncpath = PathBuf::from(&wd);
        syncpath.push("testdata");
        syncpath.push("out_scratch");
        syncpath.push("6539709be17615dbbf5d55f84f293c55ecc50abf4865374c916bef052e713fec.dat");

        match sf.mark_deleted_and_save(&conf,Some(syncpath.clone())) {
            Err(e) => panic!("Failed to write syncfile: {:?}", e),
            Ok(_) => ()
        };
        assert!(start_revguid != sf.revguid);
        let new_revguid = sf.revguid;

        let mut sf = readit(syncpath);

        assert!(sf.is_deleted);
        assert!(start_revguid != sf.revguid);
        assert_eq!(new_revguid, sf.revguid);

        // should have no data
        let mut data:Vec<u8> = Vec::new();
        match sf.decrypt_to_writer(&conf, &mut data) {
            Err(e) => panic!("Error {:?}", e),
            Ok(_) => {
                assert_eq!(0,data.len());
            }
        }
    }
}
