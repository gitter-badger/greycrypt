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
use std::io::{Read, Write, BufReader, BufRead, SeekFrom, Seek};
use self::crypto::digest::Digest;
use self::crypto::sha2::Sha256;

use self::rand::{ Rng, OsRng };
use self::rustc_serialize::base64::{ToBase64, STANDARD, FromBase64 };

const IVSIZE: usize = 16;

struct OpenFileState {
    handle: File,
    //iv: &'a [u8]
    iv: [u8;IVSIZE]
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
    pub fn get_sync_id_and_path(conf:&config::SyncConfig, nativefile: &str) -> Result<(String,PathBuf),String> {
        let (kw,relpath) = {
            let res = conf.mapping.get_kw_relpath(nativefile);
            match res {
                None => return Err(format!("No mapping found for native file: {}", nativefile)),
                Some((kw,relpath)) => (kw,relpath)
            }
        };
        let idstr = SyncFile::get_sync_id(kw,&relpath);

        let mut syncpath = PathBuf::from(&conf.sync_dir);
        let prefix = &idstr.to_string()[0..2];
        syncpath.push(prefix);
        syncpath.push(&idstr);
        syncpath.set_extension("dat");

        Ok((idstr,syncpath))
    }

    pub fn from_native(conf:&config::SyncConfig, nativefile: &str) -> Result<SyncFile,String> {
        let (kw,relpath) = {
            let res = conf.mapping.get_kw_relpath(nativefile);
            match res {
                None => return Err(format!("No mapping found for native file: {}", nativefile)),
                Some((kw,relpath)) => (kw,relpath)
            }
        };

        let idstr = SyncFile::get_sync_id(kw,&relpath);

        let is_binary = match util::file_is_binary(nativefile) {
            Err(e) => return Err(format!("Failed to check binary status: {:?}", e)),
            Ok(isb) => isb
        };

        let ret = SyncFile {
            id: idstr,
            keyword: kw.to_string(),
            relpath: relpath,
            revguid: uuid::Uuid::new_v4(),
            nativefile: nativefile.to_string(),
            is_binary: is_binary,
            sync_file_state: SyncFileState::Closed
        };

        Ok(ret)
    }

    fn read_top_lines(fin:&File,count:i32) -> Result<Vec<String>,String> {
        let mut reader = BufReader::new(fin);

        let mut lines:Vec<String> = Vec::new();
        for i in 0 .. count {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Err(e) => return Err(format!("Failed to read header line {} from syncfile: {}", i, e)),
                Ok(_) => {
                    lines.push(line.trim().to_string());
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
            Err(e) => return Err(format!("Failed to seek reader after metadata: {:?}", e)),
            Ok(_) => ()
        }

        Ok(lines)
    }

    pub fn get_syncid_from_file(conf:&config::SyncConfig, syncpath:&PathBuf) -> Result<String,String> {
        if !syncpath.is_file() {
            return Err(format!("Syncfile does not exist: {:?}", syncpath));
        }
        let fin = match File::open(syncpath.to_str().unwrap()) {
            Err(e) => return Err(format!("Can't open syncfile: {:?}: {}", syncpath, e)),
            Ok(fin) => fin
        };
        match SyncFile::read_top_lines(&fin,1) {
            Err(e) => return Err(e),
            Ok(lines) => {
                Ok(lines[0].to_string())
            }
        }
    }

    fn init_sync_read(conf:&config::SyncConfig, syncpath:&PathBuf) -> Result<(File,String,[u8;IVSIZE],HashMap<String,String>),String> {
        let key = match conf.encryption_key {
            None => return Err("No encryption key".to_string()),
            Some(k) => k
        };

        if !syncpath.is_file() {
            return Err(format!("Syncfile does not exist: {:?}", syncpath));
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
            Err(e) => return Err(format!("Can't open syncfile: {:?}: {}", syncpath, e)),
            Ok(fin) => fin
        };

        let (syncid,ivline,mdline) = {
            match SyncFile::read_top_lines(&fin,3) {
                Err(e) => return Err(e),
                Ok(lines) => {
                    (lines[0].clone(), lines[1].clone(), lines[2].clone())
                }
            }
        };

        let iv = match ivline.from_base64() {
            Err(e) => return Err(format!("Unable to parse IV line: {}", e)),
            Ok(iv) => iv
        };
        if iv.len() != IVSIZE {
            return Err(format!("Unexpected IV length: {}", iv.len()));
        }

        // make crypto helper
        let key:&[u8] = &key;
        let iv:&[u8] = &iv;
        let mut crypto = crypto_util::CryptoHelper::new(key,iv);

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
                let k = parts[0].trim();
                let v = parts[1].trim();
                mdmap.insert(k.to_lowercase(),v.to_string());
            }
            mdmap
        };

        // :(
        // http://stackoverflow.com/questions/29570607/is-there-a-good-way-to-convert-a-vect-to-an-array
        let mut iv_copy:[u8;IVSIZE] = [0;IVSIZE];
        for i in 0..IVSIZE {
            iv_copy[i] = iv[i]
        }

        Ok((fin,syncid.to_string(),iv_copy,mdmap))
    }

    pub fn get_metadata_hash(conf:&config::SyncConfig, syncpath:&PathBuf) -> Result<HashMap<String,String>,String> {
        let (_,_,_,mdmap) = match SyncFile::init_sync_read(conf,syncpath) {
            Err(e) => return Err(e),
            Ok(stuff) => stuff
        };
        Ok(mdmap)
    }

    pub fn from_syncfile(conf:&config::SyncConfig, syncpath:&PathBuf) -> Result<SyncFile,String> {
        let (fin,syncid,iv,mdmap) = match SyncFile::init_sync_read(conf,syncpath) {
            Err(e) => return Err(e),
            Ok(stuff) => stuff
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
        let is_binary = {
            match mdmap.get("is_binary") {
                None => return Err(format!("Key 'is_binary' is required in metadata")),
                Some(v) => {
                    match bool::from_str(v) {
                        Err(e) => return Err(format!("Failed to parse is_binary bool: {}", e)),
                        Ok(b) => b
                    }
                }
            }
        };

        // :(
        // http://stackoverflow.com/questions/29570607/is-there-a-good-way-to-convert-a-vect-to-an-array
        let mut iv_copy:[u8;IVSIZE] = [0;IVSIZE];
        for i in 0..IVSIZE {
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
            is_binary: is_binary,
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
        // TODO: let _ is janky
        // TODO: no panic (return on err)
        let _ = writeln!(v, "ver: {}", md_format_ver);
        let _ = writeln!(v, "kw: {}", self.keyword);
        let _ = writeln!(v, "relpath: {}", self.relpath);
        let _ = writeln!(v, "revguid: {}", self.revguid);
        let _ = writeln!(v, "is_binary: {}", self.is_binary);

        // additional fields that aren't required for sync but are helpful for resolving conflicts
        match util::get_file_mtime(&self.nativefile) {
            Err(e) => panic!("{}",e),
            Ok(mtime) => {
                let _ = writeln!(v, "origin_native_mtime: {}", mtime);
            }
        }
        let _ = writeln!(v, "origin_host: {}", util::get_hostname());
    }

    fn decrypt_helper(&mut self, conf:&config::SyncConfig, out:&mut Write) -> Result<(),String> {
        {
            let ofs = {
                match self.sync_file_state {
                    SyncFileState::Open(ref ofs) => ofs,
                    _ => return Err("Sync file not open".to_string())
                }
            };
            let key = match conf.encryption_key {
                None => return Err("No encryption key".to_string()),
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
                    Err(e) => { return Err(format!("Read error: {}", e)) },
                    Ok(num_read) => {
                        let enc_bytes = &buf[0 .. num_read];
                        let eof = num_read == 0;
                        let res = crypto.decrypt(enc_bytes, eof);
                        match res {
                            Err(e) => return Err(format!("Encryption error: {:?}", e)),
                            Ok(d) => {
                                let dlen = d.len();
                                match out.write(&d) {
                                    Err(e) => return Err(format!("Failed to write to file: {}", e)),
                                    Ok(nbytes) => {
                                        if nbytes != dlen {
                                            return Err(format!("Failed to write expected bytes: wrote {}, want {}", nbytes, dlen));
                                        }
                                    }
                                }
                            }
                        }
                        if eof {
                            match out.flush() {
                                Err(e) => return Err(format!("Failed to flush output reader: {}",e)),
                                Ok(_) => ()
                            }
                            break;
                        }
                    }
                }
            }
        }

        // close input file now
        self.sync_file_state = SyncFileState::Closed;

        Ok(())
    }

    pub fn decrypt_to_writer(&mut self, conf:&config::SyncConfig, out:&mut Write) -> Result<(),String> {
        // if file is binary, can go directly to target_out.  otherwise, have to
        // stream to intermediate buffer and nativize the line endings.

        if self.is_binary {
            self.decrypt_helper(conf,out)
        } else {
            let mut temp_out:Vec<u8> = Vec::new();
            match self.decrypt_helper(conf,&mut temp_out) {
                Err(e) => return Err(e),
                Ok(_) => {
                    //println!("dec: {:?}", String::from_utf8(temp_out.clone()).unwrap());
                    match util::decanon_lines(&temp_out) {
                        Err(e) => return Err(e),
                        Ok(temp_out) => {
                            match out.write(&temp_out) {
                                Err(e) => return Err(format!("{:?}",e)),
                                Ok(_) => ()
                            }
                        }
                    }

                    Ok(())
                }
            }
        }
    }

    pub fn restore_native(&mut self, conf:&config::SyncConfig) -> Result<String,String> {
        { // check to make sure file is open, scoped to prevent borrow conflicts
            let ofs = {
                match self.sync_file_state {
                    SyncFileState::Open(ref ofs) => ofs,
                    _ => return Err("Sync file not open".to_string())
                }
            };
        }

        let nativefile = self.nativefile.clone();
        let outpath = match nativefile.trim() {
            "" => return Err("Native path not set, call set_nativefile_path()".to_string()),
            s => s
        };

        let outpath_par = PathBuf::from(&outpath);
        let outpath_par = outpath_par.parent().unwrap();
        if !outpath_par.is_dir() {
            let res = create_dir_all(&outpath_par);
            match res {
                Err(e) => return Err(format!("Failed to create output directory: {:?}: {:?}", outpath_par, e)),
                Ok(_) => ()
            }
        }

        // prep output handles
        let res = File::create(outpath);
        let mut fout = match res {
            Err(e) => return Err(format!("Failed to create output file: {:?}: {:?}", outpath, e)),
            Ok(f) => f
        };
        //let mut fout = BufWriter::new(fout);

        match self.decrypt_to_writer(conf,&mut fout) {
            Err(e) => return Err(format!("Failed to decrypt file: {:?}: {:?}", outpath, e)),
            Ok(_) => ()
        }

        Ok(outpath.to_string())
    }

    pub fn read_native_and_save(&self, conf:&config::SyncConfig, override_path: Option<PathBuf>) -> Result<String,String> {
        let (sid,outpath) = match SyncFile::get_sync_id_and_path(conf,&self.nativefile) {
            Err(e) => return Err(format!("Can't get id/path: {:?}", e)),
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
                Err(e) => return Err(format!("Failed to create output sync directory: {:?}: {:?}", outpath_par, e)),
                Ok(_) => ()
            }
        }

        let outname = outpath.to_str().unwrap();
        let mut fout = match File::create(outname) {
            Err(e) => return Err(format!("Can't create output file: {:?}", e)),
            Ok(f) => f
        };

        let key = match conf.encryption_key {
            None => return Err(format!("No encryption key")),
            Some(k) => k
        };

        // create random iv
        let mut rng = OsRng::new().ok().unwrap();
        let mut iv: [u8; 16] = [0; 16];
        rng.fill_bytes(&mut iv);

        // make crypto helper
        let key:&[u8] = &key;
        let iv:&[u8] = &iv;
        let mut crypto = crypto_util::CryptoHelper::new(key,iv);

        // write sync id to file (unencrypted)
        // TODO: get rid of these _
        let _ = writeln!(fout, "{}", sid);

        // write iv to file (unencrypted, base64 encoded)
        let _ = writeln!(fout, "{}", iv.to_base64(STANDARD));

        // write metadata (encrypted, base64 encoded string)
        let mut v:Vec<u8> = Vec::new();
        self.pack_header(&mut v);

        // pass true to signal EOF so that the metadata can be decrypted without needing to read
        // the whole file.
        let res = crypto.encrypt(&v[..], true);

        match res {
            Err(e) => return Err(format!("Encryption error: {:?}", e)),
            Ok(d) => {
                let b64_out = d[..].to_base64(STANDARD);
                let _ = writeln!(fout, "{}", b64_out);
            }
        }

        // remake crypto helper for file data
        let mut crypto = crypto_util::CryptoHelper::new(key,iv);

        // read, encrypt, and write file data, not slurping because it could be big
        let mut fin = match File::open(&self.nativefile) {
            Err(e) => { return Err(format!("Can't open input native file: {}: {}", &self.nativefile, e)) },
            Ok(fin) => fin
        };

        if self.is_binary {
            // stream-encrypt binary files
            const SIZE: usize = 1048576;
            let mut v: Vec<u8> = vec![0;SIZE];
            let mut buf = &mut v;

            loop {
                let read_res = fin.read(&mut buf);
                match read_res {
                    Err(e) => { return Err(format!("Read error: {}", e)) },
                    Ok(num_read) => {
                        //println!("read {} bytes",num_read);
                        let enc_bytes = &buf[0 .. num_read];
                        let eof = num_read == 0;
                        let res = crypto.encrypt(enc_bytes, eof);
                        match res {
                            Err(e) => return Err(format!("Encryption error: {:?}", e)),
                            Ok(d) => {
                                let dlen = d.len();
                                match fout.write(&d) {
                                    Err(e) => return Err(format!("Failed to write to file: {}", e)),
                                    Ok(nbytes) => {
                                        if nbytes != dlen {
                                            return Err(format!("Failed to write expected bytes: wrote {}, want {}", nbytes, dlen));
                                        }
                                    }
                                }
                            }
                        }
                        //println!("encrypted {} bytes",num_read);
                        if eof {
                            break;
                        }
                    }
                }
            }
        } else {
            // for text files, read them in and normalized the line endings (use \r), so that
            // the (decrypted) binary value is same on all platforms.  this is required for de-dup
            // comparisons.  when unpacking to native on a target platform, we'll restore the
            // proper line endings
            let br = BufReader::new(fin);
            let in_lines = br.lines();
            let mut out_lines:Vec<String> = Vec::new();
            for l in in_lines {
                match l {
                    Err(e) => return Err(format!("Failed to read line from alleged text source: {}", e)),
                    Ok(l) => {
                        out_lines.push(l.to_string());
                    }
                }
            }

            let line_buf = match util::canon_lines(&out_lines) {
                Err(e) => return Err(format!("{}", e)),
                Ok(buf) => buf
            };

            let enc_bytes = &line_buf[0 .. line_buf.len()];

            match crypto.encrypt(enc_bytes, true) {
                Err(e) => return Err(format!("Encryption error: {:?}", e)),
                Ok(d) => {
                    let dlen = d.len();
                    match fout.write(&d) {
                        Err(e) => return Err(format!("Failed to write to file: {}", e)),
                        Ok(nbytes) => {
                            if nbytes != dlen {
                                return Err(format!("Failed to write expected bytes: wrote {}, want {}", nbytes, dlen));
                            }
                        }
                    }
                }
            }
        }

        Ok(outname.to_string())
    }

    pub fn create_syncfile(conf:&config::SyncConfig, nativepath:&PathBuf, override_path: Option<PathBuf>) -> Result<(String,SyncFile),String> {
        let res = SyncFile::from_native(&conf, nativepath.to_str().unwrap());
        let sf = match res {
            Err(e) => return Err(format!("Failed to create sync file: {:?}", e)),
            Ok(sf) => sf
        };

        let res = sf.read_native_and_save(&conf, override_path);
        match res {
            Err(e) => return Err(format!("Failed to update sync file with native data: {:?}", e)),
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
    use config;
    use mapping;
    use syncfile;

    extern crate toml;

    fn get_config() -> config::SyncConfig {
        let wd = env::current_dir().unwrap();

        // generate a mock mapping, with keyword "gcprojroot" mapped to the project's root dir
        let wds = wd.to_str().unwrap();
        let mapping = format!("gcprojroot = '{}'", wds);
        let mapping = toml::Parser::new(&mapping).parse().unwrap();
        let mapping = mapping::Mapping::new(&mapping).ok().expect("WTF?");

        let mut outpath = PathBuf::from(&wd);
        outpath.push("testdata");
        outpath.push("out_syncdir");

        let ec: [u8;32] = [0; 32];

        let conf = config::SyncConfig {
            sync_dir: outpath.to_str().unwrap().to_string(),
            mapping: mapping,
            encryption_key: Some(ec),
            syncdb_dir: None,
            native_paths: Vec::new()
        };
        conf
    }

    #[test]
    fn write_read_syncfile() {
        let wd = env::current_dir().unwrap();
        let mut testpath = PathBuf::from(&wd);
        testpath.push("testdata");
        testpath.push("test_native_file.txt");

        let savetp = testpath.to_str().unwrap();

        let mut conf = get_config();

        let sfpath = match syncfile::SyncFile::create_syncfile(&conf,&testpath,None) {
            Err(e) => panic!("Error {:?}", e),
            Ok((sfpath,_)) => sfpath
        };
        let sfpath = PathBuf::from(&sfpath);

        let file_syncid = match syncfile::SyncFile::get_syncid_from_file(&conf,&sfpath) {
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
                assert_eq!(sf.relpath, "/testdata/test_native_file.txt");
                // revguid could be anything, but if it wasn't a guid we would already have failed
                assert_eq!(sf.nativefile, savetp);
                // file should be open
                match sf.sync_file_state {
                    syncfile::SyncFileState::Open(ref ofs) => {
                        // assume handle is valid (will check anyway when we read data)
                        // iv should be non-null (though its possible that it could
                        // be randomly all zeros, that should be very rare)
                        let mut zcount = 0;
                        for x in 0..syncfile::IVSIZE {
                            if ofs.iv[x] == 0 { zcount = zcount + 1 }
                        }
                        assert!(zcount != syncfile::IVSIZE)
                    },
                    _ => panic!("Unexpected file state")
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
                            ex_out.push("test_native_file.txt");
                            assert_eq!(outfile, ex_out.to_str().unwrap());
                            outfile
                        }
                    }
                };

                // slurp source and output files and compare
                let srctext = util::slurp_text_file(&savetp.to_string());
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

        let mut conf = get_config();

        let sfpath = match syncfile::SyncFile::create_syncfile(&conf,&testpath,None) {
            Err(e) => panic!("Error {:?}", e),
            Ok((sfpath,_)) => sfpath
        };
        let sfpath = PathBuf::from(&sfpath);

        let res = syncfile::SyncFile::from_syncfile(&conf,&sfpath);
        match res {
            Err(e) => panic!("Error {:?}", e),
            Ok(sf) => {
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
        let conf = get_config();

        let wd = env::current_dir().unwrap();
        let mut testpath = PathBuf::from(&wd);
        testpath.push("testdata");
        testpath.push("test_native_file.txt");
        let srctext = util::slurp_bin_file(&testpath.to_str().unwrap().to_string());

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
                //     Ok(ref mut f) => { f.write(&data); () }
                // }
                assert_eq!(srctext,data);


            }
        }
    }
}
