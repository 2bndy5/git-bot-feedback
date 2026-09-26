use std::sync::{Mutex, OnceLock};

struct Logger;

static LOGS: OnceLock<Mutex<Vec<String>>> = OnceLock::new();

fn logs() -> &'static Mutex<Vec<String>> {
    LOGS.get_or_init(|| Mutex::new(Vec::new()))
}

impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        log::max_level() > metadata.level()
    }

    fn log(&self, record: &log::Record) {
        let message = if record.target() == "CI_LOG_GROUPING" {
            format!("{}", record.args())
        } else {
            format!(
                "[{:>5}]{}: {}",
                record.level().as_str(),
                record.module_path().unwrap_or_default(),
                record.args()
            )
        };
        logs().lock().unwrap().push(message.clone());
        println!("{message}");
    }

    fn flush(&self) {}
}

pub fn logger_init() {
    let _ = log::set_logger(&Logger);
}

pub fn take_logs() -> Vec<String> {
    std::mem::take(&mut *logs().lock().unwrap())
}

#[allow(dead_code, reason = "This is used by most tests but not all of them")]
#[derive(Debug, PartialEq, Default)]
pub enum EventType {
    #[default]
    Push,
    PullRequest,
}

impl std::fmt::Display for EventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Push => write!(f, "push"),
            Self::PullRequest => write!(f, "pull_request"),
        }
    }
}
