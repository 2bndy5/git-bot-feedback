#![cfg(feature = "gitlab")]
use chrono::Utc;
use git_bot_feedback::{
    RestApiClient, RestClientError, ReviewAction, ReviewComment, ReviewOptions,
    client::{GitlabApiClient, USER_AGENT},
};
use mockito::{Matcher, Mock, Server, ServerGuard};
use serde_json::json;
use std::env;

mod common;
use common::{EventType, logger_init, take_logs};

const MARKER: &str = "<!-- git-bot-feedback -->\n";
const PROJECT_ID: &str = "42";
const SHA: &str = "deadbeef";
const MR: u64 = 7;
const TOKEN: &str = "123456";
const RESET_RATE_LIMIT_HEADER: &str = "ratelimit-reset";
const REMAINING_RATE_LIMIT_HEADER: &str = "ratelimit-remaining";
const SUMMARY: &str = "This is a summary of the review.";

/// The merge request's diff.
///
/// - line 2 is added
/// - lines 1, 3, 4, and 5 are unchanged (old lines 1, 2, 3, and 4)
const RAW_DIFF: &str = "diff --git a/src/lib.rs b/src/lib.rs
index 1234567..89abcde 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,4 +1,5 @@
 line 1
+line 2
 line 3
 line 4
 line 5
";

#[derive(Debug, Clone, Copy, PartialEq)]
enum MrState {
    Open,
    Draft,
    Merged,
    Locked,
    NoDiffRefs,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Discussions {
    Happy,
    BadJson,
    HttpError,
}

struct TestParams {
    event_t: EventType,
    action: ReviewAction,
    mr_state: MrState,
    discussions: Discussions,
    delete_review_comments: bool,
    all_comments_reused: bool,
    fail_create_draft: bool,
    fail_get_drafts: bool,
    other_drafts: bool,
    approved_before: bool,
    no_token: bool,
}

impl Default for TestParams {
    fn default() -> Self {
        Self {
            event_t: EventType::PullRequest,
            action: ReviewAction::Comment,
            mr_state: MrState::Open,
            discussions: Discussions::Happy,
            delete_review_comments: false,
            all_comments_reused: false,
            fail_create_draft: false,
            fail_get_drafts: false,
            other_drafts: false,
            approved_before: false,
            no_token: false,
        }
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

fn mock_api(server: &mut ServerGuard, method: &str, path: &str) -> Mock {
    let reset_timestamp = (Utc::now().timestamp() + 60).to_string();
    server
        .mock(
            method,
            format!("/api/v4/projects/{PROJECT_ID}/merge_requests/{MR}{path}").as_str(),
        )
        .match_header("PRIVATE-TOKEN", TOKEN)
        .match_header("User-Agent", USER_AGENT)
        .with_header(REMAINING_RATE_LIMIT_HEADER, "50")
        .with_header(RESET_RATE_LIMIT_HEADER, reset_timestamp.as_str())
}

fn diff_refs() -> serde_json::Value {
    json!({"base_sha": "aaa", "head_sha": "bbb", "start_sha": "ccc"})
}

fn diff_note(id: u64, body: &str, path: &str, line: u32, resolved: bool) -> serde_json::Value {
    json!({
        "id": id,
        "type": "DiffNote",
        "body": body,
        "author": {"id": 99, "username": "project_42_bot"},
        "position": {
            "base_sha": "aaa",
            "head_sha": "bbb",
            "start_sha": "ccc",
            "position_type": "text",
            "old_path": path,
            "new_path": path,
            "old_line": null,
            "new_line": line,
        },
        "resolvable": true,
        "resolved": resolved,
    })
}

/// Existing threads of discussion in the merge request (2 pages).
fn existing_discussions(page: u8) -> serde_json::Value {
    let bot = |text: &str| format!("{MARKER}{text}");
    if page == 1 {
        json!([
            // a bot comment that is reused
            {"id": "d1", "notes": [diff_note(1, &bot("A reused comment"), "src/lib.rs", 42, false)]},
            // an outdated bot comment
            {"id": "d2", "notes": [diff_note(2, &bot("An outdated comment"), "src/lib.rs", 10, false)]},
            // a thread started by a user
            {"id": "d3", "notes": [diff_note(3, "A user's comment", "src/lib.rs", 3, false)]},
        ])
    } else {
        json!([
            // an outdated bot comment that was already resolved
            {"id": "d4", "notes": [diff_note(4, &bot("A resolved comment"), "src/lib.rs", 11, true)]},
            // an outdated bot comment with a reply from a user
            {"id": "d5", "notes": [
                diff_note(5, &bot("An outdated comment with a reply"), "src/lib.rs", 12, false),
                diff_note(6, "A user's reply", "src/lib.rs", 12, false),
            ]},
            // a bot comment that is not on a line of the diff (eg. a review summary)
            {"id": "d6", "notes": [{
                "id": 7,
                "type": null,
                "body": bot(SUMMARY),
                "author": {"id": 99, "username": "project_42_bot"},
                "resolvable": false,
                "resolved": false,
            }]},
        ])
    }
}

fn proposed_comments(all_reused: bool) -> Vec<ReviewComment> {
    let reused = ReviewComment {
        line_start: None,
        line_end: 42,
        // GitLab removes trailing whitespace from the existing comment's body
        comment: "A reused comment\n".to_string(),
        path: "src/lib.rs".to_string(),
    };
    if all_reused {
        return vec![reused];
    }
    vec![
        reused,
        // a suggestion that spans unchanged lines 3 to 5
        ReviewComment {
            line_start: Some(3),
            line_end: 5,
            comment: "Reformat\n```suggestion\nline 3\nline 4\nline 5\n```\n".to_string(),
            path: "src/lib.rs".to_string(),
        },
        // a comment on an added line
        ReviewComment {
            line_start: None,
            line_end: 2,
            comment: format!("{MARKER}Added line"),
            path: "src/lib.rs".to_string(),
        },
        // a comment about a file that is not in the diff
        ReviewComment {
            line_start: None,
            line_end: 1,
            comment: "Not in diff".to_string(),
            path: "src/other.rs".to_string(),
        },
    ]
}

async fn setup_and_run(test_params: &TestParams) {
    let mut server = Server::new_async().await;
    unsafe {
        env::set_var("CI_API_V4_URL", format!("{}/api/v4", server.url()));
        env::set_var("CI_PROJECT_ID", PROJECT_ID);
        env::set_var("CI_COMMIT_SHA", SHA);
        if test_params.event_t == EventType::PullRequest {
            env::set_var("CI_MERGE_REQUEST_IID", MR.to_string());
        } else {
            env::remove_var("CI_MERGE_REQUEST_IID");
        }
        if test_params.no_token {
            env::remove_var("GITLAB_TOKEN");
        } else {
            env::set_var("GITLAB_TOKEN", TOKEN);
        }
    }
    logger_init();
    log::set_max_level(log::LevelFilter::Debug);
    let mut client = GitlabApiClient::new().unwrap();
    client.set_user_agent(USER_AGENT).unwrap();

    let mut mocks = reject_unexpected_requests(&mut server);
    let is_mr = test_params.event_t == EventType::PullRequest && !test_params.no_token;
    if is_mr {
        let mr_info = json!({
            "iid": MR,
            "state": if test_params.mr_state == MrState::Merged { "merged" } else { "opened" },
            "draft": test_params.mr_state == MrState::Draft,
            "discussion_locked": test_params.mr_state == MrState::Locked,
            "diff_refs": if test_params.mr_state == MrState::NoDiffRefs {
                serde_json::Value::Null
            } else {
                diff_refs()
            },
        });
        // fetched once to cull reviews and once to post the review (unless culling fails)
        let fetches = if test_params.discussions == Discussions::BadJson {
            1
        } else {
            2
        };
        mocks.push(
            mock_api(&mut server, "GET", "")
                .with_body(mr_info.to_string())
                .expect(fetches)
                .create(),
        );
    }

    let can_cull = is_mr && !matches!(test_params.mr_state, MrState::Merged | MrState::Locked);
    let mut culled = false;
    if can_cull {
        for page in [1u8, 2] {
            let mut mock = mock_api(&mut server, "GET", "/discussions")
                .match_query(Matcher::UrlEncoded("page".to_string(), page.to_string()));
            if page == 1 {
                mock = mock.with_header(
                    "link",
                    format!(
                        "<{}/api/v4/projects/{PROJECT_ID}/merge_requests/{MR}/discussions?page=2&per_page=100>; rel=\"next\"",
                        server.url()
                    )
                    .as_str(),
                );
            }
            match test_params.discussions {
                Discussions::HttpError => {
                    mocks.push(mock.with_status(500).create());
                    break;
                }
                Discussions::BadJson => {
                    mocks.push(mock.with_body("not json").create());
                    break;
                }
                Discussions::Happy => {
                    mocks.push(
                        mock.with_body(existing_discussions(page).to_string())
                            .create(),
                    );
                }
            }
        }
        culled = test_params.discussions == Discussions::Happy;
    }
    if culled {
        let resolve = |server: &mut ServerGuard, id: &str| {
            mock_api(server, "PUT", format!("/discussions/{id}").as_str())
                .match_body(Matcher::Json(json!({"resolved": true})))
                .create()
        };
        let delete = |server: &mut ServerGuard, thread: &str, note: u64| {
            mock_api(
                server,
                "DELETE",
                format!("/discussions/{thread}/notes/{note}").as_str(),
            )
            .with_status(204)
            .create()
        };
        if test_params.delete_review_comments {
            mocks.push(delete(&mut server, "d2", 2));
            mocks.push(delete(&mut server, "d4", 4));
            mocks.push(delete(&mut server, "d5", 5));
            // the user's reply remains
            mocks.push(resolve(&mut server, "d5"));
        } else {
            mocks.push(resolve(&mut server, "d2"));
            mocks.push(resolve(&mut server, "d5"));
        }
    }

    let can_post = can_cull
        && test_params.mr_state != MrState::Draft
        && test_params.mr_state != MrState::NoDiffRefs
        && test_params.discussions != Discussions::BadJson;
    // the reused comment is removed when culling
    let reused = culled;
    let creates_drafts = can_post && !(test_params.all_comments_reused && reused);
    if can_post {
        let drafts = mock_api(&mut server, "GET", "/draft_notes");
        mocks.push(if test_params.fail_get_drafts {
            drafts.with_status(500).create()
        } else {
            let mut pending = vec![json!({"id": 900, "note": format!("{MARKER}A stale draft")})];
            if test_params.other_drafts {
                // a review that the token's user has in progress
                pending.push(json!({"id": 901, "note": "A user's draft"}));
            }
            drafts.with_body(json!(pending).to_string()).create()
        });
        if !test_params.fail_get_drafts {
            mocks.push(
                mock_api(&mut server, "DELETE", "/draft_notes/900")
                    .with_status(204)
                    .create(),
            );
        }
    }
    let mut drafts_created = vec![];
    if creates_drafts {
        mocks.push(
            mock_api(&mut server, "GET", "/raw_diffs")
                .match_header("Accept", "text/plain")
                .with_body(RAW_DIFF)
                .create(),
        );
        if !reused {
            // the reused comment is posted again because culling failed
            mocks.push(
                mock_api(&mut server, "POST", "/draft_notes")
                    .match_body(Matcher::Json(json!({
                        "note": format!("{MARKER}A reused comment"),
                        "position": {
                            "base_sha": "aaa",
                            "head_sha": "bbb",
                            "start_sha": "ccc",
                            "position_type": "text",
                            "old_path": "src/lib.rs",
                            "new_path": "src/lib.rs",
                            "old_line": 41,
                            "new_line": 42,
                        },
                    })))
                    .with_status(201)
                    .with_body(json!({"id": 1001, "note": "..."}).to_string())
                    .create(),
            );
            drafts_created.push(1001);
        }
        // a multi-line suggestion on unchanged lines
        mocks.push(
            mock_api(&mut server, "POST", "/draft_notes")
                .match_header("Content-Type", "application/json")
                .match_body(Matcher::Json(json!({
                    "note": format!("{MARKER}Reformat\n```suggestion:-2+0\nline 3\nline 4\nline 5\n```"),
                    "position": {
                        "base_sha": "aaa",
                        "head_sha": "bbb",
                        "start_sha": "ccc",
                        "position_type": "text",
                        "old_path": "src/lib.rs",
                        "new_path": "src/lib.rs",
                        "old_line": 4,
                        "new_line": 5,
                    },
                })))
                .with_status(if test_params.fail_create_draft { 400 } else { 201 })
                .with_body(json!({"id": 1002, "note": "..."}).to_string())
                .create(),
        );
        if !test_params.fail_create_draft {
            drafts_created.push(1002);
        }
        // a comment on an added line has no `old_line`
        mocks.push(
            mock_api(&mut server, "POST", "/draft_notes")
                .match_body(Matcher::Json(json!({
                    "note": format!("{MARKER}Added line"),
                    "position": {
                        "base_sha": "aaa",
                        "head_sha": "bbb",
                        "start_sha": "ccc",
                        "position_type": "text",
                        "old_path": "src/lib.rs",
                        "new_path": "src/lib.rs",
                        "new_line": 2,
                    },
                })))
                .with_status(201)
                .with_body(json!({"id": 1003, "note": "..."}).to_string())
                .create(),
        );
        drafts_created.push(1003);
    }
    if can_post {
        let request_changes = test_params.action == ReviewAction::RequestChanges;
        if test_params.other_drafts || test_params.fail_get_drafts {
            // only this review's drafts are published
            for id in &drafts_created {
                mocks.push(
                    mock_api(
                        &mut server,
                        "PUT",
                        format!("/draft_notes/{id}/publish").as_str(),
                    )
                    .with_status(204)
                    .create(),
                );
            }
        } else if !drafts_created.is_empty() || request_changes {
            let payload = if request_changes {
                json!({"reviewer_state": "requested_changes"})
            } else {
                json!({})
            };
            mocks.push(
                mock_api(&mut server, "POST", "/draft_notes/bulk_publish")
                    .match_body(Matcher::Json(payload))
                    .with_status(204)
                    .create(),
            );
        }
        mocks.push(
            mock_api(&mut server, "POST", "/notes")
                .match_body(Matcher::Json(json!({
                    "body": format!("{MARKER}<!-- review summary -->\n{SUMMARY}")
                })))
                .with_status(201)
                .create(),
        );
        match test_params.action {
            ReviewAction::Approve => {
                mocks.push(
                    mock_api(&mut server, "POST", "/approve")
                        .with_status(201)
                        .create(),
                );
            }
            ReviewAction::RequestChanges => {
                mocks.push(
                    mock_api(&mut server, "POST", "/unapprove")
                        .with_status(if test_params.approved_before {
                            201
                        } else {
                            404
                        })
                        .create(),
                );
            }
            ReviewAction::Comment => {}
        }
    }

    let mut options = ReviewOptions {
        action: test_params.action.clone(),
        // the marker is only prepended if needed
        summary: if test_params.action == ReviewAction::Approve {
            format!("{MARKER}{SUMMARY}")
        } else {
            SUMMARY.to_string()
        },
        comments: proposed_comments(test_params.all_comments_reused),
        marker: MARKER.to_string(),
        delete_review_comments: test_params.delete_review_comments,
        ..Default::default()
    };
    let total_comments = options.comments.len();
    let cull_result = client.cull_pr_reviews(&mut options).await;
    let post_result = if cull_result.is_ok() {
        Some(client.post_pr_review(&options).await)
    } else {
        None
    };
    for mock in mocks {
        mock.assert();
    }

    if test_params.no_token && test_params.event_t == EventType::PullRequest {
        assert!(matches!(cull_result, Err(RestClientError::EnvVar { .. })));
        return;
    }
    if test_params.discussions == Discussions::BadJson && can_cull {
        assert!(
            matches!(cull_result, Err(RestClientError::Json { .. })),
            "Expected Json error, got: {cull_result:?}"
        );
        return;
    }
    cull_result.unwrap();
    assert_eq!(
        options.comments.len(),
        if reused {
            total_comments - 1
        } else {
            total_comments
        }
    );
    let post_result = post_result.unwrap();
    if test_params.mr_state == MrState::NoDiffRefs {
        assert!(
            matches!(post_result, Err(RestClientError::MalformedEventInfo(_))),
            "Expected MalformedEventInfo error, got: {post_result:?}"
        );
        return;
    }
    post_result.unwrap();
    if creates_drafts {
        assert!(
            take_logs()
                .iter()
                .any(|l| l.contains("src/other.rs:1 because the file is not in the diff"))
        );
    }
}

#[tokio::test]
async fn comment() {
    setup_and_run(&TestParams::default()).await;
}

#[tokio::test]
async fn approve() {
    setup_and_run(&TestParams {
        action: ReviewAction::Approve,
        ..Default::default()
    })
    .await;
}

#[tokio::test]
async fn request_changes() {
    setup_and_run(&TestParams {
        action: ReviewAction::RequestChanges,
        ..Default::default()
    })
    .await;
}

#[tokio::test]
async fn request_changes_after_approval() {
    setup_and_run(&TestParams {
        action: ReviewAction::RequestChanges,
        approved_before: true,
        ..Default::default()
    })
    .await;
}

#[tokio::test]
async fn delete_outdated_comments() {
    setup_and_run(&TestParams {
        delete_review_comments: true,
        ..Default::default()
    })
    .await;
}

#[tokio::test]
async fn all_comments_reused() {
    setup_and_run(&TestParams {
        all_comments_reused: true,
        ..Default::default()
    })
    .await;
}

#[tokio::test]
async fn all_comments_reused_request_changes() {
    setup_and_run(&TestParams {
        all_comments_reused: true,
        action: ReviewAction::RequestChanges,
        ..Default::default()
    })
    .await;
}

#[tokio::test]
async fn fail_create_draft() {
    setup_and_run(&TestParams {
        fail_create_draft: true,
        ..Default::default()
    })
    .await;
}

#[tokio::test]
async fn fail_get_drafts() {
    setup_and_run(&TestParams {
        fail_get_drafts: true,
        ..Default::default()
    })
    .await;
}

#[tokio::test]
async fn other_pending_drafts() {
    setup_and_run(&TestParams {
        other_drafts: true,
        ..Default::default()
    })
    .await;
}

#[tokio::test]
async fn other_pending_drafts_request_changes() {
    setup_and_run(&TestParams {
        other_drafts: true,
        action: ReviewAction::RequestChanges,
        ..Default::default()
    })
    .await;
}

#[tokio::test]
async fn fail_get_discussions() {
    setup_and_run(&TestParams {
        discussions: Discussions::HttpError,
        ..Default::default()
    })
    .await;
}

#[tokio::test]
async fn bad_discussions() {
    setup_and_run(&TestParams {
        discussions: Discussions::BadJson,
        ..Default::default()
    })
    .await;
}

#[tokio::test]
async fn draft_mr() {
    setup_and_run(&TestParams {
        mr_state: MrState::Draft,
        ..Default::default()
    })
    .await;
}

#[tokio::test]
async fn merged_mr() {
    setup_and_run(&TestParams {
        mr_state: MrState::Merged,
        ..Default::default()
    })
    .await;
}

#[tokio::test]
async fn locked_mr() {
    setup_and_run(&TestParams {
        mr_state: MrState::Locked,
        ..Default::default()
    })
    .await;
}

#[tokio::test]
async fn no_diff_refs() {
    setup_and_run(&TestParams {
        mr_state: MrState::NoDiffRefs,
        ..Default::default()
    })
    .await;
}

#[tokio::test]
async fn push_event() {
    setup_and_run(&TestParams {
        event_t: EventType::Push,
        ..Default::default()
    })
    .await;
}

#[tokio::test]
async fn no_token() {
    setup_and_run(&TestParams {
        no_token: true,
        ..Default::default()
    })
    .await;
}
