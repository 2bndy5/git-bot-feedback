use git_bot_feedback::{RestApiClient, RestClientError, client::GithubApiClient};
use std::{env, io::Read, path::Path};
use tempfile::{NamedTempFile, tempdir};
mod common;
use common::logger_init;

const COMMENT: &str = "Some comment text";

#[derive(Debug, Default)]
struct TestParams {
    fail_summary: bool,
    absent: bool,
}

async fn append_summary(test_params: TestParams) -> String {
    let tmp_dir = tempdir().unwrap();
    logger_init();
    log::set_max_level(log::LevelFilter::Debug);
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
    let mut step_summary_content = String::new();
    match GithubApiClient::append_step_summary(COMMENT) {
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
