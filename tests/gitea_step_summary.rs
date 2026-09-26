#![cfg(feature = "gitea")]
use git_bot_feedback::{RestApiClient, RestClientError, client::GiteaApiClient};
use mockito::Server;
use std::{env, io::Read, path::Path};
use tempfile::{NamedTempFile, tempdir};
mod common;
use common::{logger_init, take_logs};

const COMMENT: &str = "Some comment text";

const REPO: &str = "2bndy5/git-bot-feedback";
const SHA: &str = "DEADBEEF";

#[derive(Debug, Default)]
struct TestParams {
    fail_summary: bool,
    absent: bool,
    with_step_summary_url: bool,
}

async fn append_summary(test_params: TestParams) -> (String, Vec<String>) {
    let tmp_dir = tempdir().unwrap();
    logger_init();
    log::set_max_level(log::LevelFilter::Debug);
    take_logs();
    let mut step_summary_path = NamedTempFile::new_in(tmp_dir.path()).unwrap();
    if test_params.absent {
        unsafe {
            env::remove_var("GITEA_STEP_SUMMARY");
        }
    } else {
        unsafe {
            env::set_var(
                "GITEA_STEP_SUMMARY",
                if test_params.fail_summary {
                    Path::new("not-a-file.txt")
                } else {
                    step_summary_path.path()
                },
            );
        }
    }
    let mut step_summary_content = String::new();

    let server = Server::new_async().await;
    unsafe {
        env::set_var("GITEA_API_URL", server.url());
        env::set_var("GITEA_ACTIONS", "true");
        env::set_var("GITEA_REPOSITORY", REPO);
        env::set_var("GITEA_SHA", SHA);
        env::set_var("CI", "true");
        env::set_var("GITEA_EVENT_NAME", "push");
        if test_params.with_step_summary_url {
            env::set_var("GITEA_SERVER_URL", "https://gitea.example.com");
            env::set_var("GITEA_RUN_ID", "1234");
        } else {
            env::remove_var("GITEA_SERVER_URL");
            env::remove_var("GITHUB_SERVER_URL");
            env::remove_var("GITEA_RUN_ID");
            env::remove_var("GITHUB_RUN_ID");
        }
    }
    let gt_client = GiteaApiClient::new().unwrap();

    match gt_client.append_step_summary(COMMENT) {
        Ok(_) => {
            step_summary_path
                .read_to_string(&mut step_summary_content)
                .unwrap();
        }
        Err(e) => {
            assert!(test_params.fail_summary);
            assert!(matches!(e, RestClientError::Io { task: _, source: _ }));
        }
    }
    (step_summary_content, take_logs())
}

#[tokio::test]
async fn fail_gh_summary() {
    let (summary, _) = append_summary(TestParams {
        fail_summary: true,
        ..Default::default()
    })
    .await;
    assert!(summary.is_empty());
}

#[tokio::test]
async fn pass_gh_summary() {
    let (summary, logs) = append_summary(TestParams::default()).await;
    assert!(summary.contains(COMMENT));
    assert!(!logs.iter().any(|log| log.contains("View step summary at")));
}

#[tokio::test]
async fn absent_gh_summary() {
    let (summary, _) = append_summary(TestParams {
        absent: true,
        ..Default::default()
    })
    .await;
    assert!(summary.is_empty());
}

#[tokio::test]
async fn pass_gh_summary_with_step_summary_url() {
    let (summary, logs) = append_summary(TestParams {
        with_step_summary_url: true,
        ..Default::default()
    })
    .await;
    assert!(summary.contains(COMMENT));
    assert!(logs.iter().any(|log| log.contains(&format!(
        "View step summary at https://gitea.example.com/{REPO}/actions/runs/1234"
    ))));
}
