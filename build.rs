//! This script adds a compile-time env var containing the compile-time's datetime.
//! This added env var is used to generate a sane default comment marker when the
//! consuming API (upstream) does not actually specify a comment marker to use.
use chrono::Local;

const ENV_VAR: &str = "COMPILE_DATETIME";

fn main() {
    let now = Local::now();
    println!(
        "cargo::rustc-env={ENV_VAR}={}",
        now.format("%b-%d-%Y_%H-%M")
    )
}
