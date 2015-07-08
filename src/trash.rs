use std::ffi::CString;
use std::ptr;

#[cfg(target_os = "macos")]
#[repr(C)]
struct FSRef {
    hidden: [u8;80]
}

#[cfg(target_os = "macos")]
#[link(name = "Foundation", kind = "framework")]
#[link(name = "CoreServices", kind = "framework")]
extern {
    // Apple has deprecated these apis. I'd use the new
    // API, but its probably already deprecated.
    fn FSPathMakeRefWithOptions(path: *const i8, opts: i32, fsref: *mut FSRef, is_directory: *mut i32) -> i32; // TODO: what is "Boolean"? "OsStatus" is intlike?
    fn FSMoveObjectToTrashSync(source: *mut FSRef, target: *mut FSRef, opts: i32) -> i32;
}

#[cfg(target_os = "macos")]
pub fn send_to_trash(f:&str) -> Result<(),String> {
    let mut fsref = FSRef { hidden: [0;80] };

    let make_ref_dont_follow_leaf_symlink = 1 as i32;
        let opts = make_ref_dont_follow_leaf_symlink;

        let path = CString::new(f).unwrap();
        let res = unsafe {
            FSPathMakeRefWithOptions(path.as_ptr(),opts, &mut fsref, ptr::null_mut())
        };
        if res != 0 {
            return Err(format!("Failed to locate file for trashing: {}; FSPathMakeRefWithOptions code: {}", f, res));
        }
        let res = unsafe {
            FSMoveObjectToTrashSync(&mut fsref, ptr::null_mut(), 0)
        };
        if res != 0 {
            return Err(format!("Failed to move file to trash: {}; FSMoveObjectToTrashSync code: {}", f, res));
        }
        Ok(())
}

pub fn do_something() {
    match send_to_trash("/Users/john/Documents/AnotherGreyGryptTestDir/atestfileMINE.txt") {
        Err(e) => panic!("{}", e),
        Ok(_) => ()
    }
}

#[cfg(test)]
mod tests {
    use trash;

    use std::env;
    use std::path::PathBuf;
    use std::fs::{File, remove_file};
    use std::io::Write;
    use std::fs::PathExt;

    #[cfg(target_os = "macos")]
    pub fn get_trash_path(f:&PathBuf) -> PathBuf {
        let home = match env::var("HOME") {
            Err(e) => panic!("No HOME env var? {:?}", e),
            Ok(h) => h
        };

        // make path to file in trash
        let mut pb = PathBuf::from(home);
        pb.push(".Trash");
        pb.push(f.file_name().unwrap());
        pb
    }

    pub fn is_in_trash(f:&PathBuf) -> bool {
        get_trash_path(&f).is_file()
    }

    pub fn remove_trash_file(trashpath:&PathBuf) {
        if trashpath.is_file() {
            match remove_file(trashpath.to_str().unwrap()) {
                Err(e) => panic!("{}",e),
                Ok(_) => ()
            }
        }
    }

    #[test]
    fn move_to_trash() {
        let wd = env::current_dir().unwrap();
        let mut testpath = PathBuf::from(&wd);
        testpath.push("testdata");
        testpath.push("Afilethatwillbetrashed.bythetrashmodule.trashme");

        remove_trash_file(&get_trash_path(&testpath));

        {
            let mut f = match File::create(&testpath.to_str().unwrap()) {
                Err(e) => panic!("{}", e),
                Ok(f) => f
            };

            match f.write_all(b"zzz") {
                Err(e) => panic!("{}", e),
                Ok(_) => ()
            }
        }

        match trash::send_to_trash(testpath.to_str().unwrap()) {
            Err(e) => panic!("{}", e),
            Ok(_) => ()
        }

        assert!(!testpath.is_file());
        assert!(is_in_trash(&testpath));
        remove_trash_file(&get_trash_path(&testpath));
        assert!(!is_in_trash(&testpath));
    }
}
