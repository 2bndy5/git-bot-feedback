use git_bot_feedback::{RestApiClient, RestClientError, client::GithubApiClient};
use mockito::Server;
use std::{env, io::Read, path::Path};
use tempfile::{NamedTempFile, tempdir};
mod common;
use common::logger_init;

const REPO: &str = "2bndy5/git-bot-feedback";
const SHA: &str = "DEADBEEF";

const COMMENT: &str = "Some comment text";

#[derive(Debug, Default)]
struct TestParams {
    fail_summary: bool,
    absent: bool,
}

async fn append_summary(test_params: TestParams) -> String {
    let tmp_dir = tempdir().unwrap();
    let mut step_summary_path = NamedTempFile::new_in(tmp_dir.path()).unwrap();
    if test_params.absent {
        unsafe {
            env::remove_var("GITHUB_STEP_SUMMARY");
        }
    } else {
        unsafe {
            env::set_var(
                "GITHUB_STEP_SUMMARY",
                if test_params.fail_summary {
                    Path::new("not-a-file.txt")
                } else {
                    step_summary_path.path()
                },
            );
        }
    }

    unsafe {
        env::set_var("GITHUB_REPOSITORY", REPO);
        env::set_var("GITHUB_SHA", SHA);
        env::set_var("CI", "true");
        env::set_var("GITHUB_EVENT_NAME", "push");
    };
    let server = Server::new_async().await;
    unsafe {
        env::set_var("GITHUB_API_URL", server.url());
    }

    env::set_current_dir(tmp_dir.path()).unwrap();
    logger_init();
    log::set_max_level(log::LevelFilter::Debug);
    let client = GithubApiClient::new().unwrap();

    let mut step_summary_content = String::new();
    match client.append_step_summary(COMMENT) {
        Ok(_) => {
            step_summary_path
                .read_to_string(&mut step_summary_content)
                .unwrap();
        }
        Err(e) => {
            assert!(test_params.fail_summary || test_params.absent);
            if test_params.absent {
                assert!(matches!(e, RestClientError::EnvVar { .. }));
            } else {
                assert!(matches!(e, RestClientError::Io { .. }));
            }
        }
    }
    step_summary_content
}

#[tokio::test]
async fn fail_gh_summary() {
    let summary = append_summary(TestParams {
        fail_summary: true,
        ..Default::default()
    })
    .await;
    assert!(summary.is_empty());
}

#[tokio::test]
async fn pass_gh_summary() {
    let summary = append_summary(TestParams::default()).await;
    assert!(summary.contains(COMMENT));
}

#[tokio::test]
async fn absent_gh_summary() {
    let summary = append_summary(TestParams {
        absent: true,
        ..Default::default()
    })
    .await;
    assert!(summary.is_empty());
}
