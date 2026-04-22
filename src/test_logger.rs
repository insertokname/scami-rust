use std::sync::RwLock;

use log::{Level, Metadata, Record};

pub(super) struct TestLogger {
    pub _logs: RwLock<String>,
}

impl TestLogger {
    pub const fn new() -> Self {
        TestLogger {
            _logs: RwLock::new(String::new()),
        }
    }
}

impl log::Log for TestLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Info
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            // println!("{}\n", record.args());
            self._logs
                .write()
                .unwrap()
                .push_str(&format!("{}\n", record.args()));
        }
    }

    fn flush(&self) {}
}
