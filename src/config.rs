use std::collections::BTreeMap;
use std::collections::HashSet;
use std::path::{PathBuf};
use std::fs::{PathExt};
use std::fmt;
use std::env;
use std::io;
use std::io::BufRead;

extern crate crypto;
use self::crypto::bcrypt_pbkdf::bcrypt_pbkdf;

extern crate toml;

use util;
use mapping;

use rpassword::read_password;

pub const KEY_SIZE: usize = 32;

#[derive(Clone)]
pub struct SyncConfig {
    sync_dir: String, // use sync_dir() to read this
    pub host_name: String,
    pub mapping: mapping::Mapping,
    pub encryption_key: Option<[u8; KEY_SIZE]>,
    pub syncdb_dir: Option<String>,
    pub native_paths: Vec<String>
}

impl fmt::Debug for SyncConfig {
   fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let ek_str = match self.encryption_key {
            None => "missing",
            Some(_) => "present (value suppressed)"
        };

        write!(f, "SyncConfig {{ sync_dir: {:?}, host_name: {:?}, mapping: {:?}, encryption_key: {}, syncdb_dir: {:?}, native_paths: {:?} }}",
            self.sync_dir,
            self.host_name,
            self.mapping,
            ek_str,
            self.syncdb_dir,
            self.native_paths)
    }
}

#[cfg(feature = "release_paths")]
pub const BUILD_PREFIX:&'static str = "rel";
#[cfg(feature = "release_paths")]
pub const IS_REL:bool = true;

#[cfg(not(feature = "release_paths"))]
pub const BUILD_PREFIX:&'static str = "dbg";
#[cfg(not(feature = "release_paths"))]
pub const IS_REL:bool = false;

impl SyncConfig {
    pub fn sync_dir(&self) -> &str {
        &self.sync_dir
    }

    pub fn new(sync_dir: String,
        host_name: String,
        mapping: mapping::Mapping,
        ek:Option<[u8;KEY_SIZE]>,
        syncdb_dir:Option<String>,
        native_paths: Vec<String>) -> Self {
            let mut pb = PathBuf::from(&sync_dir);

            // let tests omit build prefix
            if !cfg!(test) {
                //error
                pb.push(BUILD_PREFIX);
            }

            let conf = SyncConfig {
                sync_dir: pb.to_str().unwrap().to_owned(),
                host_name: host_name,
                mapping: mapping,
                encryption_key: ek,
                syncdb_dir: syncdb_dir,
                native_paths: native_paths
            };
            conf
    }
    
    pub fn with_encryption_key(&self,ek:Option<[u8;KEY_SIZE]>) -> Self {
        let myclone = self.clone();
        SyncConfig { encryption_key: ek, .. myclone } 
    } 
}

pub fn def_config_file() -> String {
    let mut file = "config".to_owned();
    if !IS_REL {
        file.push_str(".");
        file.push_str(BUILD_PREFIX);
    };
    file.push_str(".toml");
    file
}

fn is_cygwin() -> bool {
    match env::var("PATH") {
        Err(_) => false,
        Ok (p) => {
            //println!("{:?}", p);
            p.to_lowercase().contains("cygwin")
        }
    }
}

fn is_msys() -> bool {
    match env::var("MSYSTEM") {
        Err(_) => false,
        Ok(_) => true
    }
}

pub fn pw_prompt(pw_prompt_message:Option<&str>) -> String {
    let msg = match pw_prompt_message {
        None => "Enter encryption password:",
        Some(msg) => msg
    }; 
    println!("{}",msg);

    let password =
        if is_cygwin() || is_msys() {
            // read_password() explodes on these
            println!("Warning: MSYS or cygwin detected; password will echo");
            let mut line = String::new();
            let stdin = io::stdin();
            stdin.lock().read_line(&mut line).unwrap();
            line
        } else {
            read_password().unwrap()
        };
    let password = password.trim();

    if password.char_indices().count() < 6 {
        panic!("Illegal password, len < 6");
    }
    password.to_owned()
}

pub fn get_encryption_key(password:&str) -> [u8;KEY_SIZE] {
    // TODO: what should this salt be, where to store it, does it need to be protected?
    // perhaps generate it once and store it in syncdir?
    let salt = b"salt".to_vec();
    let pw_bytes = password.as_bytes();  
    
    let mut ek: [u8;KEY_SIZE] = [0; KEY_SIZE];
    bcrypt_pbkdf(pw_bytes, &salt, 5, &mut ek);
    ek    
}

// Parse the specified toml config file.  If None, parse file named by
// def_config_file() in the working directory.  Panics if there is
// anything wrong with the file.
// Will prompt for encryption password if it cannot be read from the config 
// file (release mode).

// Note: maybe should change this to return a Result instead of panicking,
// but the use of helper closures here makes it more convenient to just panic.
pub fn parse(cfgfile:Option<String>, hn_override:Option<String>, pw_prompt_message:Option<&str>) -> SyncConfig {
    let file = match cfgfile {
        None => def_config_file(),
        Some(f) => f
    };
    let toml = util::load_toml_file(&file);

    // define some helpers for the main toml table
    type TomlTable = BTreeMap<String, toml::Value>;

    let get_optional_section = |sname:&str| {
        toml.get(sname).map(|s| {
            match s.as_table() {
                None => panic!("Property '{}' must be a table, like: [{}]", sname, sname),
                Some (s) => s
            }
        })
    };

    let get_required_section = |sname:&str| {
        match toml.get(sname) {
            None => panic!("Required table [{}] not found", sname),
            Some (s) => {
                match s.as_table() {
                    None => panic!("Property '{}' must be a table, like: [{}]", sname, sname),
                    Some (s) => {
                        s
                    }
                }
            }
        }
    };

    let get_optional_string = |setting:&str, table:&TomlTable| {
        table.get(setting).map(|s| {
            match s.as_str() {
                None => panic!("{} must be a string", setting),
                Some(name) => name.trim().to_owned()
            }
        })
    };

    // load config
    let gen_sect = get_optional_section("General");

    // set host name:
    // fn argument has highest priority (unit tests hardcode it),
    // then config file,
    // then raw host name
    let hn = hn_override.unwrap_or_else(|| {
            gen_sect
            .and_then(|s| get_optional_string("HostnameOverride", s))
            .unwrap_or_else(util::get_hostname)
    });

    // in debug, allow password to be read from conf file
    let ek = {
        let password = if IS_REL {
            pw_prompt(pw_prompt_message)
        } else {
            gen_sect
            .and_then(|s| get_optional_string("Password", s))
            .unwrap_or_else(|| pw_prompt(pw_prompt_message))
        };
        get_encryption_key(&password)
    };

    let (sync_dir, native_paths, mapping) = {
        let mval = get_required_section("Mapping");

        let mut map_nicknames:HashSet<String> = HashSet::new();
        for (map_nick,hn_list) in mval {
            match hn_list.as_slice() {
                None => panic!("The value for map nick {} must be a list, like: [\"myhostname\"]", map_nick),
                Some(hn_list) => {
                    for lhn in hn_list {
                        match lhn.as_str() {
                            None => panic!("The values in map nick list {} must be strings, like: [\"myhostname\"]", map_nick),
                            Some (lhn) => {
                                if lhn == hn {
                                    map_nicknames.insert(map_nick.to_owned());
                                }
                            }
                        }
                    }
                }
            }
        }

        // host must be mapped to 1 nick
        if map_nicknames.is_empty() {
            panic!("Unable to map hostname: try adding a line to [Mapping] like: mynick = [\"{}\"]", hn);
        }
        if map_nicknames.len() > 1 {
            panic!("Too many mappings for hostname found, make sure [Mapping] contains only one relationship for host {}", hn);
        }

        let map_nick = map_nicknames.iter().nth(0).unwrap();

        // now try to find a HostDef.<hostname> object
        let hn_map_key = format!("HostDef-{}", map_nick);
        let hn_config = get_optional_section(&hn_map_key)
            .expect(&format!("No host definition found, try adding [{}]", &hn_map_key));

        // find key in hn_config and return its value; panics if not found.
        let mut hn_config = hn_config.clone();
        let get_and_remove = |hn_config:&mut BTreeMap<String, toml::Value>,key:&str| {
            let sd = match hn_config.get(key) {
                None => { panic!("No {} specified for host in {}", key, &hn_map_key) },
                Some (sd) => {
                    sd.clone()
                }
            };
            hn_config.remove(key);
            sd
        };

        let sync_dir = match get_and_remove(&mut hn_config,"SyncDir").as_str() {
            None => panic!("Value for SyncDir must be a string"),
            Some (sd) => {
                let pp = PathBuf::from(&sd);
                if !pp.is_dir() {
                    warn!("Warning: sync directory does not exist: {}", sd);
                }
                sd.to_owned()
            }
        };

        let mut native_paths:Vec<String> = Vec::new();
        match get_and_remove(&mut hn_config,"NativePaths").as_slice() {
            None => panic!("'NativePaths' must be a list of strings in host def: {}", hn_map_key),
            Some (ref paths) => {
                for p in paths.iter() {
                    match p.as_str() {
                        None => panic!("'NativePaths' must contain strings, found a non-string: {:?}", p),
                        Some(s) => {
                            native_paths.push(s.to_owned());
                        }
                    }
                }
            }
        }

        if native_paths.is_empty() {
            panic!("No NativePaths are configured, cannot continue");
        }


        // all the other key/value pairs are kw->dir mappings
        let map_count = hn_config.len();
        if map_count == 0 {
            let mut helpstr = String::new();
            helpstr.push_str("No mapping entries found for host\n");
            helpstr.push_str(&format!("Add lines to [{}] with this format: keyword = \"/path/to/local/dir\"\n", hn_map_key));
            helpstr.push_str("Note: on windows, double backslash is required: home = \"C:\\\\Users\\\\Fred\"");
            panic!(helpstr);
        }

        let mapping = match mapping::Mapping::new(&hn_config) {
            Ok(m) => m,
            Err(msg) => panic!(msg)
        };

        //println!("{:?}",mapping);

        (sync_dir, native_paths, mapping)
    };

    let c = SyncConfig::new(
        sync_dir,
        hn,
        mapping,
        Some(ek),
        None,
        native_paths
    );

    c
}
