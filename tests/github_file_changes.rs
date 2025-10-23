#![cfg(feature = "file-changes")]
use chrono::Utc;
mod common;
use common::logger_init;
use mockito::{Matcher, Server};
use tempfile::{NamedTempFile, TempDir};

use git_bot_feedback::{
    DiffHunkHeader, FileFilter, LinesChangedOnly, RestApiClient, client::GithubApiClient,
};
use std::{env, io::Write, path::Path};

#[derive(PartialEq, Default)]
enum EventType {
    #[default]
    Push,
    PullRequest,
}

#[derive(Default)]
struct TestParams {
    event_t: EventType,
    fail_serde_diff: bool,
    fail_serde_event_payload: bool,
    no_event_payload: bool,
}

const REPO: &str = "2bndy5/git-bot-feedback";
const SHA: &str = "DEADBEEF";
const PR: u8 = 42;
const TOKEN: &str = "123456";
const EVENT_PAYLOAD: &str = r#"{"number": 42}"#;
const RESET_RATE_LIMIT_HEADER: &str = "x-ratelimit-reset";
const REMAINING_RATE_LIMIT_HEADER: &str = "x-ratelimit-remaining";
const MALFORMED_RESPONSE_PAYLOAD: &str = "{\"message\":\"Resource not accessible by integration\"}";

async fn get_paginated_changes(lib_root: &Path, test_params: &TestParams) {
    let tmp = TempDir::new().expect("Failed to create a temp dir for test");
    let mut event_payload = NamedTempFile::new_in(tmp.path())
        .expect("Failed to spawn a tmp file for test event payload");
    if EventType::PullRequest == test_params.event_t
        && !test_params.fail_serde_event_payload
        && !test_params.no_event_payload
    {
        event_payload
            .write_all(EVENT_PAYLOAD.as_bytes())
            .expect("Failed to write data to test event payload file")
    }

    unsafe {
        env::set_var("GITHUB_REPOSITORY", REPO);
        env::set_var("GITHUB_SHA", SHA);
        env::set_var("GITHUB_TOKEN", TOKEN);
        env::set_var("CI", "true");
        env::set_var(
            "GITHUB_EVENT_NAME",
            if test_params.event_t == EventType::Push {
                "push"
            } else {
                "pull_request"
            },
        );
        env::set_var(
            "GITHUB_EVENT_PATH",
            if test_params.no_event_payload {
                Path::new("not_a_file.txt")
            } else {
                event_payload.path()
            },
        );
    };
    let mut server = Server::new_async().await;
    unsafe {
        env::set_var("GITHUB_API_URL", server.url());
    }

    let reset_timestamp = (Utc::now().timestamp() + 60).to_string();
    let asset_path = format!(
        "{}/tests/assets/file_changes/github",
        lib_root.to_str().unwrap()
    );

    env::set_current_dir(tmp.path()).unwrap();
    logger_init();
    log::set_max_level(log::LevelFilter::Debug);
    let gh_client = GithubApiClient::new();
    if test_params.fail_serde_event_payload || test_params.no_event_payload {
        assert!(gh_client.is_err());
        return;
    }
    let client = gh_client.unwrap();

    let mut mocks = vec![];
    let diff_end_point = format!(
        "/repos/{REPO}/{}",
        if EventType::PullRequest == test_params.event_t {
            format!("pulls/{PR}/files")
        } else {
            format!("commits/{SHA}")
        }
    );
    let pg_count = if test_params.fail_serde_diff { 1 } else { 2 };
    for pg in 1..=pg_count {
        let link = if pg == 1 {
            format!("<{}{diff_end_point}?page=2>; rel=\"next\"", server.url())
        } else {
            "".to_string()
        };
        let mut mock = server
            .mock("GET", diff_end_point.as_str())
            .match_header("Accept", "application/vnd.github.raw+json")
            .match_header("Authorization", format!("token {TOKEN}").as_str())
            .match_query(Matcher::UrlEncoded("page".to_string(), pg.to_string()))
            .with_header(REMAINING_RATE_LIMIT_HEADER, "50")
            .with_header(RESET_RATE_LIMIT_HEADER, reset_timestamp.as_str())
            .with_header("link", link.as_str());
        if test_params.fail_serde_diff {
            mock = mock.with_body(MALFORMED_RESPONSE_PAYLOAD);
        } else {
            mock = mock.with_body_from_file(format!(
                "{asset_path}/{}_files_pg{pg}.json",
                if test_params.event_t == EventType::Push {
                    "push"
                } else {
                    "pr"
                }
            ));
        }
        mocks.push(mock.create());
    }

    let log_scope = if test_params.event_t == EventType::Push {
        Some("push")
    } else {
        None
    };
    let file_filter = FileFilter::new(&["", "!src/*"], &["cpp", "hpp"], log_scope);
    let files = client
        .get_list_of_changed_files(&file_filter, &LinesChangedOnly::Off)
        .await;
    assert!(file_filter.is_file_ignored(&Path::new("./Cargo.toml")));
    match files {
        Err(e) => {
            if !test_params.fail_serde_diff {
                panic!("Failed to get changed files: {e:?}");
            }
        }
        Ok(files) => {
            assert_eq!(files.len(), 2);
            for (file, diff_ctx) in files {
                assert!(["src/demo.cpp", "src/demo.hpp"].contains(&file.as_str()));
                if file == "src/demo.hpp" {
                    let diff_hunk = DiffHunkHeader {
                        old_start: 5,
                        old_lines: 10,
                        new_start: 5,
                        new_lines: 10,
                    };
                    assert!(diff_ctx.is_hunk_in_diff(&diff_hunk).is_some());
                    let diff_hunk = DiffHunkHeader {
                        old_start: 5,
                        old_lines: 0,
                        new_start: 4,
                        new_lines: 12,
                    };
                    assert!(diff_ctx.is_hunk_in_diff(&diff_hunk).is_none());
                }
            }
        }
    }
    for mock in mocks {
        mock.assert();
    }
}

async fn test_get_changes(test_params: &TestParams) {
    let tmp_dir = TempDir::new().unwrap();
    let lib_root = env::current_dir().unwrap();
    env::set_current_dir(tmp_dir.path()).unwrap();
    get_paginated_changes(&lib_root, test_params).await;
    env::set_current_dir(lib_root.as_path()).unwrap();
    drop(tmp_dir);
}

#[tokio::test]
async fn get_push_files_paginated() {
    test_get_changes(&TestParams::default()).await
}

#[tokio::test]
async fn get_pr_files_paginated() {
    test_get_changes(&TestParams {
        event_t: EventType::PullRequest,
        ..Default::default()
    })
    .await
}

#[tokio::test]
async fn fail_push_files_paginated() {
    test_get_changes(&TestParams {
        fail_serde_diff: true,
        ..Default::default()
    })
    .await
}

#[tokio::test]
async fn fail_pr_files_paginated() {
    test_get_changes(&TestParams {
        event_t: EventType::PullRequest,
        fail_serde_diff: true,
        ..Default::default()
    })
    .await
}

#[tokio::test]
async fn fail_event_payload() {
    test_get_changes(&TestParams {
        event_t: EventType::PullRequest,
        fail_serde_event_payload: true,
        ..Default::default()
    })
    .await
}

#[tokio::test]
async fn no_event_payload() {
    test_get_changes(&TestParams {
        event_t: EventType::PullRequest,
        no_event_payload: true,
        ..Default::default()
    })
    .await
}
