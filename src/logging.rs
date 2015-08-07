use std::collections::HashSet;
use std::env;
use log;
use log::{LogRecord, LogMetadata};

pub struct SimpleLogger;

impl log::Log for SimpleLogger {
    fn enabled(&self, _: &LogMetadata) -> bool {
        true
    }

    fn log(&self, record: &LogRecord) {
        if self.enabled(record.metadata()) {
            println!("{} - {}", record.level(), record.args());
        }
    }        
}

#[derive(Clone)]
pub struct LoggerUtil {
    pub warnonce_messages: HashSet<String>
}

impl LoggerUtil {
    pub fn warn_once(&mut self, message:&str) {
        if !self.warnonce_messages.contains(message) {
            warn!("{}", message);
            self.warnonce_messages.insert(message.to_owned());
        }
    }   
}

pub fn init(level: Option<log::LogLevelFilter>) -> Result<LoggerUtil, log::SetLoggerError> {   
    let level = match level {
        None => log::LogLevelFilter::Info,
        Some(level) => level  
    };
    
    // allow RUST_LOG env var to change to level - if set, it takes precendence over input parameter.
    // this duplicates functionality from the env_logger, but I don't like the env_logger's format
    // and there currently isn't a way to change it.
    let level = match env::var("RUST_LOG") {
        Ok(val) => match val.trim().to_lowercase().as_ref() {
            "info" => log::LogLevelFilter::Info,
            "trace" => log::LogLevelFilter::Trace,
            "debug" => log::LogLevelFilter::Debug,
            "error" => log::LogLevelFilter::Error,
            _ => panic!("unknown log level: {}", val)
        },
        Err(_) => level
    };
    
    // The logger must only be initialized once per process, else it panics.  
    // This Effs up the tests.  So here is a crude check that makes sure 
    // we init only once.  Can init LoggerUtil as much as we want, though.    
    // NOTE: this _still_ sometimes fails because the tests threads don't 
    // synchronize and can thus try to init at the same time.  Probably need 
    // some kind of global mutex here - reuse the process mutex? But really 
    // the test framework should allow some kind of one time init.
    if log_enabled!(log::LogLevel::Info) || log_enabled!(log::LogLevel::Error) {             
        return Ok(LoggerUtil {
            warnonce_messages: HashSet::new()
        })
    }
    
    let res = log::set_logger(|max_log_level| {
        max_log_level.set(level);
        
        Box::new(SimpleLogger)
    });
     
    match res {
        Err(e) => Err(e),
        Ok(_) => Ok(LoggerUtil {
            warnonce_messages: HashSet::new()
        })
    }
}
