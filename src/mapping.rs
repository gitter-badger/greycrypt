use std::collections::HashMap;
use std::collections::BTreeMap;
use std::path::{Path};

extern crate toml;

use util;

pub struct Mapping {
    dir_to_keyword: HashMap<String,String>,
    keyword_to_dir: HashMap<String,String>
}

impl Mapping {
    pub fn new(toml_src: &BTreeMap<String, toml::Value>) -> Result<Mapping,String> {
        // build the dir/keyword mapping and reverse mapping
        // keys in both cases are stored uppercase, for insensitivity

        let mut ret = Mapping {
            dir_to_keyword: HashMap::new(),
            keyword_to_dir: HashMap::new()
        };

        // input toml is keyword->dir
        for (keyword,dir) in toml_src {
            let dir = match dir.as_str() {
                None => return Err(format!("Invalid mapping value for keyword {}; value {:?} should be a directory (String)", keyword, dir).to_string()),
                Some(v) => v
            };
            let keyword = keyword.to_uppercase();
            // preserve case on dir in this map; it isn't an error if the dir doesn't exist,
            // since we may not have synced it yet.
            ret.keyword_to_dir.insert(keyword.to_owned(),dir.to_owned());

            let dir = dir.to_uppercase();
            ret.dir_to_keyword.insert(dir,keyword);

            //println!("kd: {:?}; dk: {:?}",ret.keyword_to_dir,ret.dir_to_keyword);
        }
        Ok(ret)
    }

    pub fn lookup_kw(&self, nativedir: &str) -> Option<&str> {
        let nativedir = nativedir.to_uppercase();
        let res = self.dir_to_keyword.get(&nativedir);
        match res {
            None => None,
            Some (s) => Some(s)
        }
    }

    pub fn lookup_dir(&self, keyword: &str) -> Option<&String> {
        let keyword = keyword.to_uppercase();
        let res = self.keyword_to_dir.get(&keyword);
        res
    }

    pub fn get_kw_relpath(&self, nativefile: &str) -> Option<(&str,String)> {
        // walk nativepath directories backwards, looking for a mapping.
        let mut walk = Path::new(nativefile).parent();
        let mut res = None;

        loop {
            match walk {
                None => break,
                Some(p) => {
                    let ps = match p.to_str() {
                        None => panic!("Failed to unpack path, possibly not valid: {:?}", p),
                        Some(p) => p
                    };

                    let kw = self.lookup_kw(ps);
                    match kw {
                        None => {
                            walk = p.parent();
                            continue
                        }
                        Some (kw) => {
                            // find the relpath
                            let relpath = &nativefile[ps.len()..].to_owned();
                            let relpath = util::canon_path(relpath);
                            if kw.is_empty() {
                                panic!("Empty mapped keyword for path: {}", nativefile);
                            }
                            if relpath.is_empty() {
                                panic!("Empty relpath for path: {}", nativefile);
                            }                            
                            res = Some((kw,relpath));
                            break;
                        }
                    }
                }
            }
        }

        res
    }
}

#[cfg(test)]
mod tests {
    use config;
    use testlib;
              
    fn test_kw_to_dir(config:&config::SyncConfig, kw: &str, expected: &str) {
        let expect = format!("No {} key", kw);
        assert_eq!(config.mapping.keyword_to_dir.get(kw).expect(&expect), expected);
    }
    fn test_dir_to_kw(config:&config::SyncConfig, dir:&str, expected: &str) {
        let expect = format!("No {} key", dir);
        assert_eq!(config.mapping.dir_to_keyword.get(dir).expect(&expect), expected);
    }
    
    #[test]
    #[cfg(not(target_os = "windows"))]
    fn parse_mapping() {
        let config = testlib::util::get_test_config();
        
        assert_eq!(config.mapping.dir_to_keyword.len(), 1);
        assert_eq!(config.mapping.keyword_to_dir.len(), 1);

        test_kw_to_dir(&config, "HOME", "/Users/john");
        test_dir_to_kw(&config, "/USERS/JOHN", "HOME");
        assert_eq!(config.mapping.dir_to_keyword.get("/Users/john"), None); // canon paths are upcase
        //assert_eq!(config.mapping.keyword_to_dir.get("HOME").expect("No HOME key"), "C:\\Users\\John");
        //assert_eq!(config.mapping.dir_to_keyword.get("C:\\USERS\\JOHN").expect("No dir key"), "HOME");
        //assert_eq!(config.mapping.dir_to_keyword.get("C:\\Users\\John"), None);
    }

    fn test_kw_relpath(config:&config::SyncConfig, srcpath:&str, ex_kw:&str,ex_relpath:&str) {
        let res = config.mapping.get_kw_relpath(srcpath);
        match res {
            None => panic!("Expected a keyword and relpath"),
            Some((kw,relpath)) => {
                assert_eq!(kw, ex_kw);
                assert_eq!(relpath, ex_relpath);
            }
        }
    }

    // unfortunately I need unix and windows variants of these tests, because PathBuf, which I use 
    // extensively, cannot parse paths that aren't native to the platform.
    
    #[test]
    #[cfg(not(target_os = "windows"))]
    fn get_kw_relpath() {
        let config = testlib::util::get_test_config();
        test_kw_relpath(&config, "/Users/john/Documents/GreyCryptTestSrc/Another file.txt", "HOME", "/Documents/GreyCryptTestSrc/Another file.txt");

        let res = config.mapping.get_kw_relpath("/Users/Fred/Documents/GreyCryptTestSrc/Another file.txt");
        assert_eq!(res,None);
    }
    
    //"C:\\Users\\John\\Documents\\GreyCryptTestSrc\\Another file.txt"
    //"HOME"
    // "/Documents/GreyCryptTestSrc/Another file.txt"
    
    //"C:\\Users\\Fred\\Documents\\GreyCryptTestSrc\\Another file.txt"
    
    
}
