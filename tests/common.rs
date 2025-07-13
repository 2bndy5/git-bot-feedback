struct Logger;
impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        log::max_level() > metadata.level()
    }

    fn log(&self, record: &log::Record) {
        println!("{}: {}", record.level().as_str(), record.args());
    }

    fn flush(&self) {}
}

pub fn logger_init() {
    let _ = log::set_logger(&Logger);
}
