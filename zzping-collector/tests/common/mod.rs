// This file will hold test utilities shared across integration tests.
// Initially, it will contain a logger for capturing log output.
use log::{Level, LevelFilter, Log, Metadata, Record, SetLoggerError};
use std::sync::{Arc, Mutex};

/// A simple logger that captures log messages into a shared vector.
pub struct VectorLogger {
    log_messages: Arc<Mutex<Vec<String>>>,
}

impl Log for VectorLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Info
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            let msg = format!("{}", record.args());
            // We only care about connection messages for our test.
            if msg.starts_with("Attempting to connect") || msg.starts_with("Session ended") {
                self.log_messages.lock().unwrap().push(msg);
            }
        }
    }

    fn flush(&self) {}
}

impl VectorLogger {
    /// Initializes the global logger with a `VectorLogger` instance.
    pub fn init(log_messages: Arc<Mutex<Vec<String>>>) -> Result<(), SetLoggerError> {
        // Use a LevelFilter that is at least as verbose as the logger's own filter.
        let logger = Box::new(VectorLogger { log_messages });
        log::set_boxed_logger(logger)?;
        log::set_max_level(LevelFilter::Info);
        Ok(())
    }
}
