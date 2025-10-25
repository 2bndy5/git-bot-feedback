struct Logger;
impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        log::max_level() > metadata.level()
    }

    fn log(&self, record: &log::Record) {
        if record.target() == "CI_LOG_GROUPING" {
            println!("{}", record.args());
        } else {
            println!(
                "[{:>5}]{}: {}",
                record.level().as_str(),
                record.module_path().unwrap_or_default(),
                record.args()
            );
        }
    }

    fn flush(&self) {}
}

pub fn logger_init() {
    let _ = log::set_logger(&Logger);
}
