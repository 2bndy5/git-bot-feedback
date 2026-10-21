#![cfg(all(feature = "file-changes", feature = "gitlab"))]
use chrono::Utc;
mod common;
use common::{EventType, logger_init, take_logs};
use mockito::{Matcher, Mock, Server, ServerGuard};
use serde_json::json;
use tempfile::TempDir;

use git_bot_feedback::{
    FileFilter, LinesChangedOnly, RestApiClient, RestClientError,
    client::{GitlabApiClient, init_client},
};
use std::{env, path::Path};

const PROJECT_ID: &str = "42";
const SHA: &str = "deadbeef";
const MR: u64 = 7;
const TOKEN: &str = "123456";
const RESET_RATE_LIMIT_HEADER: &str = "ratelimit-reset";
const REMAINING_RATE_LIMIT_HEADER: &str = "ratelimit-remaining";
const ASSET_PATH: &str = "tests/assets/file_changes/gitea/patch.diff";

#[derive(Default)]
struct TestParams {
    event_t: EventType,
    fail_request: bool,
    bad_json: bool,
}

fn set_env(api_url: &str, event_t: &EventType) {
    unsafe {
        env::set_var("GITLAB_CI", "true");
        env::set_var("CI_API_V4_URL", api_url);
        env::set_var("CI_PROJECT_ID", PROJECT_ID);
        env::set_var("CI_COMMIT_SHA", SHA);
        env::set_var("GITLAB_TOKEN", TOKEN);
        if *event_t == EventType::PullRequest {
            env::set_var("CI_PIPELINE_SOURCE", "merge_request_event");
            env::set_var("CI_MERGE_REQUEST_IID", MR.to_string());
        } else {
            env::set_var("CI_PIPELINE_SOURCE", "push");
            env::remove_var("CI_MERGE_REQUEST_IID");
        }
        env::remove_var("GITEA_ACTIONS");
        env::remove_var("GITHUB_ACTIONS");
    }
}

/// Mocks that fail the test if any request is not expected by other mocks.
///
/// These must be created before other mocks.
fn reject_unexpected_requests(server: &mut ServerGuard) -> Vec<Mock> {
    ["GET", "POST", "PUT", "DELETE"]
        .into_iter()
        .map(|method| server.mock(method, Matcher::Any).expect(0).create())
        .collect()
}

/// A page of a commit's diff, as returned by GitLab.
fn commit_diff_page(page: u8) -> serde_json::Value {
    if page == 1 {
        json!([
            {
                "old_path": "src/main.cpp",
                "new_path": "src/main.cpp",
                "new_file": false,
                "renamed_file": false,
                "deleted_file": false,
                "diff": "@@ -1,3 +1,4 @@\n #include <stdio.h>\n+#include <string>\n \n int main() {\n",
                "collapsed": false,
                "too_large": false
            },
            {
                "old_path": "src/huge.cpp",
                "new_path": "src/huge.cpp",
                "new_file": true,
                "renamed_file": false,
                "deleted_file": false,
                "diff": "",
                "collapsed": false,
                "too_large": true
            },
            {
                "old_path": "src/removed.cpp",
                "new_path": "src/removed.cpp",
                "new_file": false,
                "renamed_file": false,
                "deleted_file": true,
                "diff": "@@ -1 +0,0 @@\n-int x = 0;\n"
            }
        ])
    } else {
        json!([
            {
                "old_path": "src/old_name.hpp",
                "new_path": "src/new_name.hpp",
                "new_file": false,
                "renamed_file": true,
                "deleted_file": false,
                "diff": ""
            },
            {
                "old_path": "docs/index.md",
                "new_path": "docs/index.md",
                "new_file": false,
                "renamed_file": false,
                "deleted_file": false,
                "diff": "@@ -1 +1 @@\n-# Docs\n+# Documentation\n"
            }
        ])
    }
}

async fn get_changes(
    lib_root: &Path,
    test_params: &TestParams,
    lines_changed_only: LinesChangedOnly,
) {
    let mut server = Server::new_async().await;
    set_env(
        format!("{}/api/v4", server.url()).as_str(),
        &test_params.event_t,
    );
    logger_init();
    log::set_max_level(log::LevelFilter::Debug);
    let client = GitlabApiClient::new().unwrap();
    assert_eq!(
        client.is_pr_event(),
        test_params.event_t == EventType::PullRequest
    );
    let reset_timestamp = (Utc::now().timestamp() + 60).to_string();

    let mut mocks = reject_unexpected_requests(&mut server);
    if test_params.event_t == EventType::PullRequest {
        let mut mock = server
            .mock(
                "GET",
                format!("/api/v4/projects/{PROJECT_ID}/merge_requests/{MR}/raw_diffs").as_str(),
            )
            .match_header("Accept", "text/plain")
            .match_header("PRIVATE-TOKEN", TOKEN)
            .with_header(REMAINING_RATE_LIMIT_HEADER, "50")
            .with_header(RESET_RATE_LIMIT_HEADER, reset_timestamp.as_str());
        if test_params.fail_request {
            mock = mock
                .with_status(404)
                .with_body(r#"{"message":"404 Not found"}"#);
        } else {
            mock = mock.with_body_from_file(format!("{}/{ASSET_PATH}", lib_root.to_str().unwrap()));
        }
        mocks.push(mock.create());
    } else {
        let endpoint = format!("/api/v4/projects/{PROJECT_ID}/repository/commits/{SHA}/diff");
        for page in [1u8, 2] {
            let mut mock = server
                .mock("GET", endpoint.as_str())
                .match_header("PRIVATE-TOKEN", TOKEN)
                .match_query(Matcher::AllOf(vec![
                    Matcher::UrlEncoded("page".to_string(), page.to_string()),
                    Matcher::UrlEncoded("per_page".to_string(), "100".to_string()),
                ]))
                .with_header(REMAINING_RATE_LIMIT_HEADER, "50")
                .with_header(RESET_RATE_LIMIT_HEADER, reset_timestamp.as_str());
            if page == 1 {
                mock = mock.with_header(
                    "link",
                    format!(
                        "<{}{endpoint}?page=2&per_page=100>; rel=\"next\", <{}{endpoint}?page=1&per_page=100>; rel=\"first\"",
                        server.url(),
                        server.url()
                    )
                    .as_str(),
                );
            }
            if test_params.fail_request {
                mocks.push(mock.with_status(403).create());
                break;
            }
            if test_params.bad_json {
                mocks.push(mock.with_body("not json").create());
                break;
            }
            mocks.push(mock.with_body(commit_diff_page(page).to_string()).create());
        }
    }

    let file_filter = if test_params.event_t == EventType::PullRequest {
        FileFilter::new(&["", "!src/**"], &["rs", "yml"], None)
    } else {
        FileFilter::new(&[], &["cpp", "hpp"], None)
    };
    let files = client
        .get_list_of_changed_files(&file_filter, &lines_changed_only, None, false)
        .await;
    for mock in mocks {
        mock.assert();
    }
    if test_params.fail_request {
        assert!(
            matches!(files, Err(RestClientError::RequestContext { .. })),
            "Expected RequestContext error, got: {files:?}"
        );
        return;
    }
    if test_params.bad_json {
        assert!(
            matches!(files, Err(RestClientError::Json { .. })),
            "Expected Json error, got: {files:?}"
        );
        return;
    }
    let files = files.unwrap();
    if test_params.event_t == EventType::PullRequest {
        // same expectations as the Gitea test that uses this asset
        assert_eq!(files.len(), 10);
        assert!(files.keys().all(|f| f.starts_with("src/")));
        assert!(files.contains_key("src/lib.rs"));
        return;
    }
    let mut names = files.keys().cloned().collect::<Vec<_>>();
    names.sort();
    match lines_changed_only {
        LinesChangedOnly::Off => {
            assert_eq!(names, ["src/huge.cpp", "src/main.cpp", "src/new_name.hpp"]);
        }
        _ => assert_eq!(names, ["src/main.cpp"]),
    }
    assert_eq!(files["src/main.cpp"].added_lines, vec![2]);
    assert!(
        take_logs()
            .iter()
            .any(|l| l.contains("GitLab omitted the changes to src/huge.cpp"))
    );
}

async fn test_get_changes(test_params: &TestParams, lines_changed_only: LinesChangedOnly) {
    let tmp_dir = TempDir::new().unwrap();
    let lib_root = env::current_dir().unwrap();
    env::set_current_dir(tmp_dir.path()).unwrap();
    get_changes(&lib_root, test_params, lines_changed_only).await;
    env::set_current_dir(lib_root.as_path()).unwrap();
}

#[tokio::test]
async fn mr_raw_diff() {
    test_get_changes(
        &TestParams {
            event_t: EventType::PullRequest,
            ..Default::default()
        },
        LinesChangedOnly::Off,
    )
    .await;
}

#[tokio::test]
async fn mr_raw_diff_unavailable() {
    test_get_changes(
        &TestParams {
            event_t: EventType::PullRequest,
            fail_request: true,
            ..Default::default()
        },
        LinesChangedOnly::Off,
    )
    .await;
}

#[tokio::test]
async fn push_paginated() {
    test_get_changes(&TestParams::default(), LinesChangedOnly::Off).await;
}

#[tokio::test]
async fn push_paginated_lines_changed_only() {
    test_get_changes(&TestParams::default(), LinesChangedOnly::On).await;
}

#[tokio::test]
async fn push_request_fails() {
    test_get_changes(
        &TestParams {
            fail_request: true,
            ..Default::default()
        },
        LinesChangedOnly::Off,
    )
    .await;
}

#[tokio::test]
async fn push_bad_json() {
    test_get_changes(
        &TestParams {
            bad_json: true,
            ..Default::default()
        },
        LinesChangedOnly::Off,
    )
    .await;
}

#[test]
fn init_gitlab_client() {
    set_env("https://gitlab.example.com/api/v4", &EventType::PullRequest);
    unsafe {
        env::set_var("CI_DEBUG_TRACE", "true");
    }
    let client = init_client().unwrap();
    assert_eq!(client.client_kind(), "gitlab");
    assert!(client.is_pr_event());
    assert!(client.is_debug_enabled());
    assert_eq!(client.event_name().unwrap(), "merge_request_event");
}

#[test]
fn bad_merge_request_iid() {
    set_env("https://gitlab.example.com/api/v4", &EventType::PullRequest);
    unsafe {
        env::set_var("CI_MERGE_REQUEST_IID", "not-a-number");
    }
    assert!(matches!(
        GitlabApiClient::new(),
        Err(RestClientError::MalformedEventInfo(_))
    ));
}

#[test]
fn missing_api_url() {
    set_env("https://gitlab.example.com/api/v4", &EventType::Push);
    unsafe {
        env::remove_var("CI_API_V4_URL");
    }
    assert!(matches!(
        GitlabApiClient::new(),
        Err(RestClientError::EnvVar { .. })
    ));
}
