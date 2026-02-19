pub mod review_comments;
pub mod thread_comments;

pub const DEFAULT_MARKER: &str = concat!(
    "<!-- ",
    env!("CARGO_CRATE_NAME"),
    "/",
    env!("CARGO_PKG_VERSION"),
    "/",
    env!("COMPILE_DATETIME"), // env var set by build.rs
    " -->\n"
);
