use std::ptr;

#[cfg(target_os = "windows")]
use std::os::windows::ffi::OsStrExt;
#[cfg(target_os = "windows")]
use std::ffi::OsStr;
#[cfg(target_os = "windows")]
extern crate winapi;

#[cfg(target_os = "macos")]
use std::ffi::CString;

#[cfg(target_os = "windows")]
#[repr(C)]
struct SHFILEOPSTRUCTW {
    hwnd: winapi::HWND,
    w_func: winapi::UINT,
    p_from: winapi::LPCWSTR,
    p_to: winapi::LPCWSTR,
    f_flags: u32,
    f_any_operations_aborted: winapi::BOOL,
    h_name_mappings: winapi::LPVOID,
    lpsz_progress_title: winapi::LPCWSTR
}

#[cfg(target_os = "windows")]
#[link(name = "shell32")]
extern "stdcall" {
    fn SHFileOperationW(lp_file_op: *mut SHFILEOPSTRUCTW) -> i32;
}

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

#[cfg(target_os = "windows")]
pub fn send_to_trash(f:&str) -> Result<(),String> {
    const FO_DELETE:winapi::UINT = 3;
    const FOF_SILENT:u32 = 4;
    const FOF_NOCONFIRMATION:u32 = 16;
    const FOF_ALLOWUNDO:u32 = 64;
    const FOF_NOERRORUI:u32 = 1024;

    let path:Vec<u16> = OsStr::new(f).encode_wide().chain(Some(0)).collect::<Vec<_>>();

    let mut fileop = SHFILEOPSTRUCTW {
        hwnd: ptr::null_mut(),
        w_func: FO_DELETE,
        p_from: path.as_ptr(),
        p_to: ptr::null_mut(),
        f_flags: FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_SILENT,
        f_any_operations_aborted: 0,
        h_name_mappings: ptr::null_mut(),
        lpsz_progress_title: ptr::null()
    };

    let res = unsafe {
        SHFileOperationW(&mut fileop)
    };
    if res != 0 {
        return Err(format!("Failed to send file to recycle bin: {}; SHFileOperationW code: {}", f, res));
    } else {
        return Ok(())
    }
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

    #[cfg(target_os = "windows")]
    pub fn get_trash_path(f:&PathBuf) -> PathBuf {
        // not easily possible on windows, since the RB isn't a real folder
        //PathBuf::new()
        panic!("I can't do that, Dave");
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

        // can't really clean out RB on windows
        if !cfg!(target_os = "windows") {
            remove_trash_file(&get_trash_path(&testpath));
        }

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

        if !cfg!(target_os = "windows") {
            assert!(is_in_trash(&testpath));
            remove_trash_file(&get_trash_path(&testpath));
            assert!(!is_in_trash(&testpath));
        }

    }
}
