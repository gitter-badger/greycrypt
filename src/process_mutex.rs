#[cfg(not(target_os = "windows"))]
use std::path::{PathBuf};

#[cfg(not(target_os = "windows"))]
extern crate nix;

#[cfg(not(target_os = "windows"))]
use self::nix::fcntl::*;
#[cfg(not(target_os = "windows"))]
use self::nix::fcntl::FcntlArg::{F_SETLK};
#[cfg(not(target_os = "windows"))]
use self::nix::sys::stat::{S_IWUSR};
#[cfg(not(target_os = "windows"))]
use self::nix::unistd;
#[cfg(not(target_os = "windows"))]
use std::os::unix::io::RawFd;

use util;

#[derive(Debug)]
#[cfg(not(target_os = "windows"))]
pub struct ProcessMutex {
	handle: RawFd
}

#[cfg(not(target_os = "windows"))]
fn create_mutex(name:&str) -> Result<ProcessMutex,String> {
	let path = format!("/tmp/{}", name);

	let pb = PathBuf::from(&path);

	let cmf = || {
		let flags = O_CREAT | O_WRONLY;
		let res = open(&pb, flags, S_IWUSR);

		let fd = match res {
			Err(_) => {
				//println!("Failed to open");
				return res;
			}
			Ok(fd) => fd
		};

		let fl = flock {
			// TODO: use proper constants for these...when available
			l_type: 3, // F_WRLCK
			l_whence: 0, //SEEK_SET,
			l_start: 0,
			l_len: 0,
			l_pid: unistd::getpid(),
			l_sysid: 0
		};

		//println!("excl lock");
		let res = fcntl(fd, F_SETLK(&fl));
		match res {
			Err(_) => res,
			Ok(_) => Ok(fd)
		}
	};

	let f = match cmf() {
		Err(e) => {
			let ne = format!("Failed to create/lock file '{}', another greycrypt instance may be running.  Code: {:?}", path, e);
			return Err(ne)
		},
		Ok(f) => f
	};

	Ok(ProcessMutex {
		handle: f
	})
}

#[cfg(target_os = "windows")]
use std::ptr;
#[cfg(target_os = "windows")]
use std::ffi::OsStr;
#[cfg(target_os = "windows")]
use std::os::windows::ffi::OsStrExt;
#[cfg(target_os = "windows")]
extern crate winapi;

#[cfg(target_os = "windows")]
#[link(name = "kernel32")]
extern "stdcall" {
    fn CreateMutexW(
        lp_mutex_attributes: winapi::LPVOID, // actually its a struct pointer, but I always pass NULL, so with LPVOID I don't have to define the struct
        b_initial_owner: winapi::BOOL,
        lp_name: winapi::LPCWSTR) -> winapi::HANDLE;
	fn GetLastError() -> winapi::DWORD;
}

#[derive(Debug)]
#[cfg(target_os = "windows")]
pub struct ProcessMutex {
	handle: winapi::HANDLE
}

#[cfg(target_os = "windows")]
fn create_mutex(name:&str) -> Result<ProcessMutex,String> {
    let name:Vec<u16> = OsStr::new(name).encode_wide().chain(Some(0)).collect::<Vec<_>>();

    let (err,mutie) = unsafe {
        let mutie = CreateMutexW(ptr::null_mut(), winapi::TRUE, name.as_ptr());
		let err = GetLastError();
		(err,mutie)
    };
    if mutie == ptr::null_mut() || err != 0 {
        Err(format!("failed to create mutex, another greycrypt instance may be running.  Res: {:?}; Code: {:?}", mutie, err))
    } else {
		Ok(ProcessMutex {
			handle: mutie
		})
	}
}

pub fn acquire(name:&str) -> Result<ProcessMutex,String> {
	// convert name into a filename
	let name = util::canon_path(name);
	let name = name.replace("/", "_")
		.replace(":", "_")
		.replace(" ", "_");
	let name = format!("greycrypt_mutex_{}", name);

	create_mutex(&name)
}
