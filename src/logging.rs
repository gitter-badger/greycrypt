use std::collections::HashSet;

use log;
use log::{LogRecord, LogLevel, LogMetadata};

pub struct SimpleLogger;

impl log::Log for SimpleLogger {
    fn enabled(&self, metadata: &LogMetadata) -> bool {
        metadata.level() <= LogLevel::Info
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

impl SimpleLogger {
    pub fn init() -> Result<LoggerUtil, log::SetLoggerError> {   
        // The logger must only be initialized once per process, 
        // else it panics.  This Effs up the tests.  So here 
        // is a crude check that makes sure we init only once.
        // Can init LoggerUtil as much as we want, though.
        
        // NOTE: this _still_ sometimes fails because the tests threads don't synchronize
        // and can thus try to init at the same time.  Probably need some kind of global mutex here -
        // reuse the process mutex? But really the test framework should allow some kind of one time init.
        if log_enabled!(log::LogLevel::Info) {             
            return Ok(LoggerUtil {
                warnonce_messages: HashSet::new()
            })
        }
        
        let res = log::set_logger(|max_log_level| {
            max_log_level.set(log::LogLevelFilter::Info);
            
            Box::new(SimpleLogger)
        });
         
        match res {
            Err(e) => Err(e),
            Ok(_) => Ok(LoggerUtil {
                warnonce_messages: HashSet::new()
            })
        }
    }
}