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
            ret.keyword_to_dir.insert(keyword.to_string(),dir.to_string());

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
                    let ps = p.to_str().unwrap();

                    let kw = self.lookup_kw(ps);
                    match kw {
                        None => {
                            walk = p.parent();
                            continue
                        }
                        Some (kw) => {
                            // find the relpath
                            let relpath = &nativefile[ps.len()..].to_string();
                            let relpath = util::canon_path(relpath);
                            // TODO: validate that kw, relpath have non-zero length
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

    #[test]
    fn parse_mapping() {
        let config = config::parse();
        assert_eq!(config.mapping.dir_to_keyword.len(), 1);
        assert_eq!(config.mapping.keyword_to_dir.len(), 1);

        assert_eq!(config.mapping.keyword_to_dir.get("HOME").expect("No HOME key"), "C:\\Users\\John");
        assert_eq!(config.mapping.dir_to_keyword.get("C:\\USERS\\JOHN").expect("No dir key"), "HOME");
        assert_eq!(config.mapping.dir_to_keyword.get("C:\\Users\\John"), None);
    }

    #[test]
    fn test_get_kw_relpath() {
        let config = config::parse();
        let res = config.mapping.get_kw_relpath("C:\\Users\\John\\Documents\\GreyCryptTestSrc\\Another file.txt");
        match res {
            None => panic!("Expected a keyword and relpath"),
            Some((kw,relpath)) => {
                assert_eq!(kw, "HOME");
                assert_eq!(relpath, "/Documents/GreyCryptTestSrc/Another file.txt");
            }
        }

        let res = config.mapping.get_kw_relpath("C:\\Users\\Fred\\Documents\\GreyCryptTestSrc\\Another file.txt");
        assert_eq!(res,None);
    }
}
