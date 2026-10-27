//! Tests that the Gitea client works with the environment set by Forgejo's runner.
//!
//! Forgejo's runner sets each variable as both `FORGEJO_*` and `GITHUB_*`
//! (including `GITHUB_ACTIONS`), but sets no `GITEA_*` variables.
#![cfg(feature = "gitea")]
use git_bot_feedback::{OutputVariable, client::init_client};
use std::{env, fs, path::PathBuf};
use tempfile::tempdir;

const REPO: &str = "2bndy5/git-bot-feedback";
const SHA: &str = "DEADBEEF";
const WORKSPACE: &str = "/workspace/2bndy5/git-bot-feedback";

/// Set the environment variables that Forgejo's runner sets.
///
/// The `GITHUB_*` copies are given different values so the tests can tell which one is used.
fn set_forgejo_env(output: &str, step_summary: &str) {
    unsafe {
        for name in [
            "GITEA_ACTIONS",
            "GITEA_REPOSITORY",
            "GITEA_SHA",
            "GITEA_EVENT_NAME",
            "GITEA_API_URL",
            "GITEA_WORKSPACE",
            "GITEA_OUTPUT",
            "GITEA_STEP_SUMMARY",
            "GITEA_TOKEN",
        ] {
            env::remove_var(name);
        }
        for (name, value) in [
            ("ACTIONS", "true"),
            ("REPOSITORY", REPO),
            ("SHA", SHA),
            ("EVENT_NAME", "push"),
            ("API_URL", "https://forgejo.example.com/api/v1"),
            ("WORKSPACE", WORKSPACE),
            ("OUTPUT", output),
            ("STEP_SUMMARY", step_summary),
        ] {
            env::set_var(format!("FORGEJO_{name}"), value);
            let github_value = match name {
                "ACTIONS" | "API_URL" => value.to_string(),
                _ => format!("{value}.github"),
            };
            env::set_var(format!("GITHUB_{name}"), github_value);
        }
    }
}

#[test]
fn forgejo_runner_env() {
    let tmp_dir = tempdir().unwrap();
    let output = tmp_dir.path().join("output.txt");
    let step_summary = tmp_dir.path().join("SUMMARY.md");
    fs::write(&output, "").unwrap();
    fs::write(&step_summary, "").unwrap();
    set_forgejo_env(output.to_str().unwrap(), step_summary.to_str().unwrap());

    // Forgejo's runner also sets `GITHUB_ACTIONS`, but the Gitea client is used.
    let client = init_client().unwrap();
    assert_eq!(client.client_kind(), "gitea");
    assert!(!client.is_pr_event());
    assert_eq!(
        client.workspace_root().unwrap(),
        Some(PathBuf::from(WORKSPACE))
    );

    client
        .write_output_variables(&[OutputVariable {
            name: "VAR".to_string(),
            value: "value".to_string(),
        }])
        .unwrap();
    assert_eq!(fs::read_to_string(&output).unwrap(), "VAR=value\n");

    client.append_step_summary("summary").unwrap();
    assert_eq!(fs::read_to_string(&step_summary).unwrap(), "\nsummary\n\n");
}

#[test]
fn forgejo_api_url_is_required() {
    let tmp_dir = tempdir().unwrap();
    set_forgejo_env(
        tmp_dir.path().join("output.txt").to_str().unwrap(),
        tmp_dir.path().join("SUMMARY.md").to_str().unwrap(),
    );
    unsafe {
        env::remove_var("FORGEJO_API_URL");
        env::remove_var("GITHUB_API_URL");
    }
    let err = init_client().err().unwrap();
    assert!(
        err.to_string()
            .contains("FORGEJO_API_URL or GITHUB_API_URL"),
        "unexpected error: {err}"
    );
}
