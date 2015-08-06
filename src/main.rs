#![feature(plugin)]
#![plugin(clippy)]

#![feature(path_ext)]
#![feature(append)] // for sync dedup, hopefully can remove

#[macro_use]
extern crate log;

mod util;
mod config;
mod mapping;
mod syncfile;
mod crypto_util;
mod syncdb;
mod core;
mod commands;
mod trash;
mod logging;
mod process_mutex;

#[cfg(test)]
mod testlib;

use std::thread;

extern crate getopts;
extern crate rpassword;

use getopts::Options;
use std::env;

fn print_usage(program: &str, opts: Options) {
    let brief = format!("Usage: {} [options]", program);
    print!("{}", opts.usage(&brief));
}

fn main() {
    let log_util = match logging::SimpleLogger::init() {
        Err(e) => panic!("Failed to init logger: {}", e),
        Ok(l) => l
    };

    // parse command line
    let args: Vec<String> = env::args().collect();
    let program = args[0].clone();
    let mut opts = Options::new();
    opts.optopt("s", "", "show syncfile metadata", "SYNC_FILE_PATH");
    opts.optopt("t", "", "set poll interval (in seconds)", "POLL_INTERVAL");
    opts.optflag("x", "", "show syncfile metadata for all conflicted files");
    opts.optopt("c", "", "use a different configuration file", "CONFIG_FILE_PATH");
    opts.optflag("h", "help", "print this help menu");
    let matches = match opts.parse(&args[1..]) {
        Ok(m) => { m }
        Err(f) => { panic!(f.to_string()) }
    };
    if matches.opt_present("h") {
        print_usage(&program, opts);
        return;
    }

    let cfile = {
        if matches.opt_present("c") {
            match matches.opt_str("c") {
                None => return print_usage(&program,opts),
                Some (i) => {
                    info!("Using configuration file: {}", i);
                    Some (i.to_owned())
                }
            }
        } else {
            None
        }
    };

    // init conf and state
    let conf = config::parse(cfile,None);
    let syncdb = match syncdb::SyncDb::new(&conf) {
        Err(e) => panic!("Failed to create syncdb: {:?}", e),
        Ok(sdb) => sdb
    };

    let mutex = match process_mutex::acquire(conf.sync_dir()) {
        Err(e) => panic!("Failed to create process mutex: {}", e),
        Ok(f) => f
    };
    let _ = mutex; // "fix" unused var warning, we just want it to hang out until it goes out of scope

    let mut state = core::SyncState::new(conf,syncdb,log_util);

    let poll_interval =
        if matches.opt_present("t") {
            match matches.opt_str("t") {
                None => return print_usage(&program,opts),
                Some (i) => match u32::from_str_radix(&i,10) {
                    Err(e) => panic!("Unable to parse poll interval: {}", e),
                    Ok(i) => i
                }
            }
        } else {
            3
        };

    // process args
    if matches.opt_present("s") {
        // inspect syncfile
        match matches.opt_str("s") {
            None => return print_usage(&program,opts),
            Some (filename) => {
                commands::show_syncfile_meta(&mut state,&filename);
            }

        }
    }
    else if matches.opt_present("x") {
        state.sync_files_for_id = core::find_all_syncfiles(&mut state);
        commands::show_conflicted_syncfile_meta(&mut state);
    }
    else {
        // run standard sync in loop
        info!("Starting");
        info!("Poll interval: {} seconds", poll_interval);

        loop {
            core::do_sync(&mut state);
            thread::sleep_ms(poll_interval * 1000);
        }
    }
}
