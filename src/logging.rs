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