#![cfg(all(feature = "file-changes", feature = "github"))]
use chrono::Utc;
mod common;
use common::{EventType, logger_init, take_logs};
use mockito::{Matcher, Server};
use tempfile::{NamedTempFile, TempDir};

use git_bot_feedback::{
    DiffHunkHeader, FileFilter, LinesChangedOnly, RestClientError, client::init_client,
};
use std::{env, io::Write, path::Path};

#[derive(Default)]
struct TestParams {
    event_t: EventType,
    /// Use the `pull_request_target` event (instead of `pull_request`).
    pr_target: bool,
    fail_serde_diff: bool,
    fail_serde_event_payload: bool,
    no_event_payload: bool,
    fail_request: bool,
}

const REPO: &str = "2bndy5/git-bot-feedback";
const SHA: &str = "DEADBEEF";
const PR: u8 = 42;
const TOKEN: &str = "123456";
const EVENT_PAYLOAD: &str =
    r#"{"pull_request": {"draft": false, "state": "open", "number": 42, "locked": false}}"#;
const RESET_RATE_LIMIT_HEADER: &str = "x-ratelimit-reset";
const REMAINING_RATE_LIMIT_HEADER: &str = "x-ratelimit-remaining";
const MALFORMED_RESPONSE_PAYLOAD: &str = "{\"message\":\"Resource not accessible by integration\"}";
const USER_AGENT: &str = "abc/123";

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
        env::set_var("GITHUB_ACTIONS", "true");
        env::remove_var("GITEA_ACTIONS");
        env::set_var("GITHUB_REPOSITORY", REPO);
        env::set_var("GITHUB_SHA", SHA);
        env::set_var("GITHUB_TOKEN", TOKEN);
        env::set_var("CI", "true");
        env::set_var(
            "GITHUB_EVENT_NAME",
            if test_params.event_t == EventType::Push {
                "push"
            } else if test_params.pr_target {
                "pull_request_target"
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
    let mut client = match init_client() {
        Ok(c) => c,
        Err(e) => {
            if test_params.fail_serde_event_payload {
                assert!(matches!(e, RestClientError::Json { .. }));
                return;
            } else if test_params.no_event_payload {
                assert!(matches!(e, RestClientError::Io { .. }));
                return;
            } else {
                panic!("Unexpected error creating GithubApiClient: {e}");
            }
        }
    };
    client.set_user_agent(USER_AGENT).unwrap();

    let mut mocks = vec![];
    let diff_end_point = format!(
        "/repos/{REPO}/{}",
        if EventType::PullRequest == test_params.event_t {
            format!("pulls/{PR}/files")
        } else {
            format!("commits/{SHA}")
        }
    );
    let pg_count = if test_params.fail_serde_diff || test_params.fail_request {
        1
    } else {
        2
    };
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
            .match_header("user-agent", USER_AGENT)
            .match_query(Matcher::UrlEncoded("page".to_string(), pg.to_string()))
            .with_header(REMAINING_RATE_LIMIT_HEADER, "50")
            .with_header(RESET_RATE_LIMIT_HEADER, reset_timestamp.as_str())
            .with_header("link", link.as_str());
        if test_params.fail_serde_diff {
            mock = mock.with_body(MALFORMED_RESPONSE_PAYLOAD);
        } else if test_params.fail_request {
            mock = mock.with_status(404).with_body(MALFORMED_RESPONSE_PAYLOAD);
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
    let file_filter = FileFilter::new(&["", "!src/**"], &["cpp", "hpp"], log_scope);
    assert!(file_filter.is_file_ignored(Path::new("./Cargo.toml")));
    let files = client
        .get_list_of_changed_files(&file_filter, &LinesChangedOnly::Off, None, false)
        .await;
    match files {
        Err(e) => {
            if test_params.fail_request {
                assert!(
                    matches!(
                        e,
                        RestClientError::Request(_)
                            | RestClientError::RequestContext { task: _, source: _ }
                    ),
                    "Expected Request error, got: {e:?}"
                );
            } else if !test_params.fail_serde_diff {
                panic!("Failed to get changed files: {e:?}");
            }
        }
        Ok(files) => {
            let expected_files: &[&str] = if test_params.event_t == EventType::Push {
                &["src/demo.cpp", "src/demo.hpp", "src/no_patch.cpp"]
            } else {
                &["src/demo.cpp", "src/demo.hpp"]
            };
            assert_eq!(files.len(), expected_files.len());
            assert!(expected_files.iter().all(|f| files.contains_key(*f)));
            for (file, diff_ctx) in files {
                assert!(file.starts_with("src/"));
                if file == "src/demo.hpp" {
                    let diff_hunk = DiffHunkHeader {
                        old_start: 5,
                        old_lines: 10,
                        new_start: 5,  // don't care
                        new_lines: 10, // don't care
                    };
                    assert!(diff_ctx.is_hunk_in_diff(&diff_hunk).is_some());
                    let diff_hunk = DiffHunkHeader {
                        old_start: 5, // don't care
                        old_lines: 0, // this is why we don't care
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
async fn get_pr_target_files_paginated() {
    test_get_changes(&TestParams {
        event_t: EventType::PullRequest,
        pr_target: true,
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

#[tokio::test]
async fn fail_request() {
    test_get_changes(&TestParams {
        event_t: EventType::PullRequest,
        fail_request: true,
        ..Default::default()
    })
    .await
}

/// The commit that the branch pointed to before the push event.
const BEFORE: &str = "FACADE";

#[derive(Default)]
struct PushParams {
    /// The value of `before` in the push event's payload.
    before: Option<&'static str>,
    /// Respond to the comparison of commits with a 404 status.
    compare_not_found: bool,
    /// Respond to the comparison of commits with a malformed payload.
    compare_malformed: bool,
    /// Respond to the comparison of commits with as many files as GitHub will list.
    compare_at_limit: bool,
}

/// Get the changed files of a push event, and return their names (sorted) with the logs.
///
/// The comparison of commits is expected to be requested only when
/// [`PushParams::before`] names a commit.
/// The pushed commit's own changes are expected to be requested only as a fallback.
async fn get_pushed_changes(
    params: &PushParams,
) -> (Result<Vec<String>, RestClientError>, Vec<String>) {
    let lib_root = env::current_dir().unwrap();
    let asset_path = format!(
        "{}/tests/assets/file_changes/github",
        lib_root.to_str().unwrap()
    );
    let tmp = TempDir::new().unwrap();
    let mut event_payload = NamedTempFile::new_in(tmp.path()).unwrap();
    if let Some(before) = params.before {
        let payload = serde_json::json!({ "before": before, "after": SHA });
        event_payload
            .write_all(payload.to_string().as_bytes())
            .unwrap();
    } else {
        event_payload.write_all(b"{}").unwrap();
    }
    let mut server = Server::new_async().await;
    unsafe {
        env::set_var("GITHUB_ACTIONS", "true");
        env::remove_var("GITEA_ACTIONS");
        env::set_var("GITHUB_REPOSITORY", REPO);
        env::set_var("GITHUB_SHA", SHA);
        env::set_var("GITHUB_TOKEN", TOKEN);
        env::set_var("CI", "true");
        env::set_var("GITHUB_EVENT_NAME", "push");
        env::set_var("GITHUB_EVENT_PATH", event_payload.path());
        env::set_var("GITHUB_API_URL", server.url());
    }
    logger_init();
    log::set_max_level(log::LevelFilter::Debug);
    take_logs();
    let client = init_client().unwrap();

    let reset_timestamp = (Utc::now().timestamp() + 60).to_string();
    let is_compare_expected = params.before.is_some_and(|sha| sha == BEFORE);
    let mut compare_mock = server
        .mock(
            "GET",
            format!("/repos/{REPO}/compare/{BEFORE}...{SHA}").as_str(),
        )
        .match_header("Authorization", format!("token {TOKEN}").as_str())
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("per_page".to_string(), "1".to_string()),
            Matcher::UrlEncoded("page".to_string(), "1".to_string()),
        ]))
        .with_header(REMAINING_RATE_LIMIT_HEADER, "50")
        .with_header(RESET_RATE_LIMIT_HEADER, reset_timestamp.as_str())
        // The comparison's other pages only list more commits. They must not be requested.
        .with_header(
            "link",
            format!(
                "<{}/repos/{REPO}/compare/{BEFORE}...{SHA}?per_page=1&page=2>; rel=\"next\"",
                server.url()
            )
            .as_str(),
        )
        .expect(if is_compare_expected { 1 } else { 0 });
    compare_mock = if params.compare_not_found {
        compare_mock
            .with_status(404)
            .with_body(r#"{"message":"Not Found","status":"404"}"#)
    } else if params.compare_malformed {
        compare_mock.with_body(MALFORMED_RESPONSE_PAYLOAD)
    } else if params.compare_at_limit {
        let files = (0..300)
            .map(|i| {
                serde_json::json!({
                    "filename": format!("src/file{i}.cpp"),
                    "status": "added",
                    "changes": 1,
                    "patch": "@@ -0,0 +1 @@\n+int x;",
                })
            })
            .collect::<Vec<_>>();
        compare_mock.with_body(serde_json::json!({ "files": files }).to_string())
    } else {
        compare_mock.with_body_from_file(format!("{asset_path}/compare_files.json"))
    };
    let compare_mock = compare_mock.create();

    let is_fallback_expected = !is_compare_expected || params.compare_not_found;
    let commit_mock = server
        .mock("GET", format!("/repos/{REPO}/commits/{SHA}").as_str())
        .match_query(Matcher::UrlEncoded("page".to_string(), "1".to_string()))
        .with_header(REMAINING_RATE_LIMIT_HEADER, "50")
        .with_header(RESET_RATE_LIMIT_HEADER, reset_timestamp.as_str())
        .with_body_from_file(format!("{asset_path}/push_files_pg2.json"))
        .expect(if is_fallback_expected { 1 } else { 0 })
        .create();

    let file_filter = FileFilter::new(&[], &["cpp", "hpp"], None);
    let result = client
        .get_list_of_changed_files(&file_filter, &LinesChangedOnly::Off, None, false)
        .await
        .map(|files| {
            let mut names = files.into_keys().collect::<Vec<_>>();
            names.sort();
            names
        });
    compare_mock.assert();
    commit_mock.assert();
    (result, take_logs())
}

/// The files changed by any of the pushed commits, not just the last commit.
const PUSHED_FILES: [&str; 3] = ["src/demo.cpp", "src/demo.hpp", "src/renamed.cpp"];
/// The files changed by the last pushed commit.
const LAST_COMMIT_FILES: [&str; 1] = ["src/demo.hpp"];

#[tokio::test]
async fn get_push_files_of_all_commits() {
    let (files, logs) = get_pushed_changes(&PushParams {
        before: Some(BEFORE),
        ..Default::default()
    })
    .await;
    assert_eq!(files.unwrap(), PUSHED_FILES);
    assert!(!logs.iter().any(|l| l.contains("Only using the changes")));
}

#[tokio::test]
async fn get_push_files_of_new_branch() {
    for before in [
        // no commit before the push
        Some("0000000000000000000000000000000000000000"),
        // not a commit SHA
        Some("../../other/repo"),
        Some(""),
        // not in the event payload
        None,
    ] {
        let (files, _) = get_pushed_changes(&PushParams {
            before,
            ..Default::default()
        })
        .await;
        assert_eq!(files.unwrap(), LAST_COMMIT_FILES);
    }
}

#[tokio::test]
async fn get_push_files_without_comparison() {
    let (files, logs) = get_pushed_changes(&PushParams {
        before: Some(BEFORE),
        compare_not_found: true,
        ..Default::default()
    })
    .await;
    assert_eq!(files.unwrap(), LAST_COMMIT_FILES);
    assert!(
        logs.iter()
            .any(|l| l.contains(format!("Only using the changes of commit {SHA}").as_str()))
    );
}

#[tokio::test]
async fn get_push_files_at_comparison_limit() {
    let (files, logs) = get_pushed_changes(&PushParams {
        before: Some(BEFORE),
        compare_at_limit: true,
        ..Default::default()
    })
    .await;
    assert_eq!(files.unwrap().len(), 300);
    assert!(
        logs.iter()
            .any(|l| l.contains("Some files changed by this push may be missing"))
    );
}

#[tokio::test]
async fn fail_push_files_comparison() {
    let (files, _) = get_pushed_changes(&PushParams {
        before: Some(BEFORE),
        compare_malformed: true,
        ..Default::default()
    })
    .await;
    assert!(matches!(files, Err(RestClientError::Json { .. })));
}
