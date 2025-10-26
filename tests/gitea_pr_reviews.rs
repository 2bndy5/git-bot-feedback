#![cfg(feature = "gitea")]
use chrono::Utc;
use git_bot_feedback::{
    RestApiClient, RestClientError, ReviewAction, ReviewComment, ReviewOptions,
    client::{GiteaApiClient, USER_AGENT},
};
use mockito::{Matcher, Server};
use std::{env, fs, io::Write, path::Path};
use tempfile::{NamedTempFile, TempDir};

mod common;
use common::{EventType, logger_init};

const MARKER: &str = "<!-- git-bot-feedback -->\n";
const SHA: &str = "deadbeef";
const REPO: &str = "2bndy5/git-bot-feedback";
const PR: i64 = 46;
const TOKEN: &str = "123456";
const OUTDATED_REVIEW_ID: i64 = 2518109626;
const REUSED_REVIEW_ID: i64 = 2519970027;
const MOCK_ASSETS_PATH: &str = "tests/assets/reviews/gitea";

const ASSET_REVIEWS_OUTDATED_PG1: &str = "reviews_outdated_pg1.json";
const ASSET_REVIEWS_NO_COMMENTS_PG1: &str = "reviews_no_comments_pg1.json";
const ASSET_REVIEWS_REUSED_PG2: &str = "reviews_reused_pg2.json";
const ASSET_REVIEW_COMMENTS_OUTDATED_PG1: &str = "review_comments_outdated_pg1.json";
const ASSET_REVIEW_COMMENTS_REUSED_PG1: &str = "review_comments_reused_pg1.json";
const ASSET_REVIEW_COMMENTS_REUSED_PG2: &str = "review_comments_reused_pg2.json";
const ASSET_REVIEW_COMMENTS_REUSED_ALL: &str = "review_comments_reused_all.json";

const RESET_RATE_LIMIT_HEADER: &str = "x-ratelimit-reset";
const REMAINING_RATE_LIMIT_HEADER: &str = "x-ratelimit-remaining";

#[derive(Clone, Copy, Debug, PartialEq)]
enum ExistingReviews {
    Happy,
    BadJson,
    HttpError,
    None,
}

#[derive(Debug)]
struct TestParams {
    event_t: EventType,
    existing_reviews: ExistingReviews,
    is_draft: bool,
    is_locked: bool,
    delete_review_comments: bool,
    fail_dismissal: bool,
    fail_resolve_comment: bool,
    fail_get_review_comments: bool,
    review_with_no_comments: bool,
    paginate_review_comments: bool,
    no_token: bool,
}

impl Default for TestParams {
    fn default() -> Self {
        Self {
            event_t: EventType::PullRequest,
            existing_reviews: ExistingReviews::Happy,
            is_draft: false,
            is_locked: false,
            delete_review_comments: false,
            fail_dismissal: false,
            fail_resolve_comment: false,
            fail_get_review_comments: false,
            review_with_no_comments: false,
            paginate_review_comments: false,
            no_token: false,
        }
    }
}

#[derive(Default)]
struct TestControlVars {
    new_review_comments: Vec<ReviewComment>,
    outdated_comment_ids: Vec<i64>,
    outdated_review_ids: Vec<i64>,
}

impl TestControlVars {
    fn mark_review_outdated(&mut self, review_id: i64) {
        self.outdated_review_ids.push(review_id);
    }

    /// Aggregate review comments returned by the review-specific endpoint.
    fn aggregate_review_comments(&mut self, review_id: i64, comments: &[serde_json::Value]) {
        let mut keep_review = false;
        for comment in comments {
            let body = comment["body"].as_str().unwrap();
            if !body.starts_with(MARKER) {
                continue;
            }
            if body.ends_with("reused bot comment") {
                self.new_review_comments.push(ReviewComment {
                    comment: body.strip_prefix(MARKER).unwrap().to_string(),
                    line_start: comment["old_position"]
                        .as_i64()
                        .and_then(|pos| (pos > 0).then_some(pos as u32)),
                    line_end: comment["new_position"].as_i64().unwrap() as u32,
                    path: comment["path"].as_str().unwrap().to_string(),
                });
                keep_review = true;
            } else {
                self.outdated_comment_ids
                    .push(comment["id"].as_i64().unwrap());
            }
        }

        if !keep_review {
            self.outdated_review_ids.push(review_id);
        }
    }
}

fn asset_path(lib_root: &Path, file_name: &str) -> String {
    format!(
        "{}/{MOCK_ASSETS_PATH}/{file_name}",
        lib_root.to_str().unwrap()
    )
}

fn load_json_array(lib_root: &Path, file_name: &str) -> Vec<serde_json::Value> {
    let fixture = fs::read_to_string(asset_path(lib_root, file_name)).unwrap();
    serde_json::from_str::<Vec<serde_json::Value>>(&fixture).unwrap()
}

async fn setup_and_run(lib_root: &Path, test_params: &TestParams) {
    unsafe {
        env::set_var("GITEA_EVENT_NAME", test_params.event_t.to_string().as_str());
        env::set_var("GITEA_REPOSITORY", REPO);
        env::set_var("GITEA_SHA", SHA);
        if test_params.no_token {
            env::remove_var("GITEA_TOKEN");
        } else {
            env::set_var("GITEA_TOKEN", TOKEN);
        }
        env::set_var("CI", "true");
        if env::var("ACTIONS_STEP_DEBUG").is_err() {
            env::set_var("ACTIONS_STEP_DEBUG", "true");
        }
    }

    let mut event_payload_path = None;
    if test_params.event_t == EventType::PullRequest {
        let mut event_payload_file = NamedTempFile::new_in("./").unwrap();
        let event_payload = serde_json::json!({
            "pull_request": {
                "draft": test_params.is_draft,
                "state": "open",
                "number": PR,
                "locked": test_params.is_locked,
            }
        })
        .to_string();
        event_payload_file
            .write_all(event_payload.as_bytes())
            .expect("Failed to create mock event payload.");
        unsafe {
            env::set_var("GITEA_EVENT_PATH", event_payload_file.path());
        }
        event_payload_path = Some(event_payload_file);
    } else {
        unsafe {
            env::remove_var("GITEA_EVENT_PATH");
        }
    }

    let reset_timestamp = (Utc::now().timestamp() + 60).to_string();
    let mut server = Server::new_async().await;
    unsafe {
        env::set_var("GITEA_API_URL", server.url());
    }

    logger_init();
    log::set_max_level(log::LevelFilter::Debug);
    let _event_payload_path = event_payload_path;
    let mut client = GiteaApiClient::new().unwrap();
    assert!(client.debug_enabled);
    assert_eq!(
        client.is_pr_event(),
        test_params.event_t == EventType::PullRequest
    );
    client.set_user_agent(USER_AGENT).unwrap();

    let mut mocks = vec![];
    let review_url_path = format!("/repos/{REPO}/pulls/{PR}/reviews");

    let mut test_control_vars = TestControlVars {
        new_review_comments: vec![
            ReviewComment {
                line_start: None,
                line_end: 42,
                comment: "A new comment (without prepended marker)".to_string(),
                path: "src/lib.rs".to_string(),
            },
            ReviewComment {
                line_start: Some(40),
                line_end: 42,
                comment: format!("{MARKER}A new comment (with prepended marker)"),
                path: "src/lib.rs".to_string(),
            },
        ],
        ..Default::default()
    };

    let summary = "This is a summary of the PR review.".to_string();
    if test_params.event_t == EventType::PullRequest
        && !test_params.no_token
        && !test_params.is_locked
        && !test_params.is_draft
    {
        match test_params.existing_reviews {
            ExistingReviews::Happy => {
                let review_pg1_asset = if test_params.review_with_no_comments {
                    ASSET_REVIEWS_NO_COMMENTS_PG1
                } else {
                    ASSET_REVIEWS_OUTDATED_PG1
                };
                let review_pg2_asset = ASSET_REVIEWS_REUSED_PG2;

                mocks.push(
                    server
                        .mock("GET", review_url_path.as_str())
                        .match_header("User-Agent", USER_AGENT)
                        .match_header("Accept", "application/json")
                        .match_header("Authorization", format!("token {TOKEN}").as_str())
                        .match_body(Matcher::Any)
                        .match_query(Matcher::UrlEncoded("page".to_string(), "1".to_string()))
                        .with_header(REMAINING_RATE_LIMIT_HEADER, "50")
                        .with_header(RESET_RATE_LIMIT_HEADER, reset_timestamp.as_str())
                        .with_header(
                            "link",
                            format!("<{}{review_url_path}?page=2>; rel=\"next\"", server.url())
                                .as_str(),
                        )
                        .with_body_from_file(asset_path(lib_root, review_pg1_asset).as_str())
                        .create(),
                );

                mocks.push(
                    server
                        .mock("GET", review_url_path.as_str())
                        .match_header("User-Agent", USER_AGENT)
                        .match_header("Accept", "application/json")
                        .match_header("Authorization", format!("token {TOKEN}").as_str())
                        .match_body(Matcher::Any)
                        .match_query(Matcher::UrlEncoded("page".to_string(), "2".to_string()))
                        .with_header(REMAINING_RATE_LIMIT_HEADER, "50")
                        .with_header(RESET_RATE_LIMIT_HEADER, reset_timestamp.as_str())
                        .with_body_from_file(asset_path(lib_root, review_pg2_asset).as_str())
                        .create(),
                );

                if test_params.review_with_no_comments {
                    test_control_vars.mark_review_outdated(OUTDATED_REVIEW_ID);
                } else if test_params.fail_get_review_comments {
                    mocks.push(
                        server
                            .mock(
                                "GET",
                                format!("{review_url_path}/{OUTDATED_REVIEW_ID}/comments").as_str(),
                            )
                            .match_header("Accept", "application/json")
                            .match_header("Authorization", format!("token {TOKEN}").as_str())
                            .with_header(REMAINING_RATE_LIMIT_HEADER, "50")
                            .with_header(RESET_RATE_LIMIT_HEADER, reset_timestamp.as_str())
                            .with_status(500)
                            .with_body("TEST CONDITION TRIGGERED")
                            .create(),
                    );
                    test_control_vars.mark_review_outdated(OUTDATED_REVIEW_ID);
                } else {
                    let outdated_comments_asset = ASSET_REVIEW_COMMENTS_OUTDATED_PG1;
                    let outdated_review_comments =
                        load_json_array(lib_root, outdated_comments_asset);
                    mocks.push(
                        server
                            .mock(
                                "GET",
                                format!("{review_url_path}/{OUTDATED_REVIEW_ID}/comments").as_str(),
                            )
                            .match_header("Accept", "application/json")
                            .match_header("Authorization", format!("token {TOKEN}").as_str())
                            .with_header(REMAINING_RATE_LIMIT_HEADER, "50")
                            .with_header(RESET_RATE_LIMIT_HEADER, reset_timestamp.as_str())
                            .with_body_from_file(
                                asset_path(lib_root, outdated_comments_asset).as_str(),
                            )
                            .create(),
                    );
                    test_control_vars
                        .aggregate_review_comments(OUTDATED_REVIEW_ID, &outdated_review_comments);
                }

                if test_params.paginate_review_comments {
                    let reused_review_comments_pg1 =
                        load_json_array(lib_root, ASSET_REVIEW_COMMENTS_REUSED_PG1);
                    let reused_review_comments_pg2 =
                        load_json_array(lib_root, ASSET_REVIEW_COMMENTS_REUSED_PG2);

                    mocks.push(
                        server
                            .mock(
                                "GET",
                                format!("{review_url_path}/{REUSED_REVIEW_ID}/comments").as_str(),
                            )
                            .match_header("Accept", "application/json")
                            .match_header("Authorization", format!("token {TOKEN}").as_str())
                            .with_header(REMAINING_RATE_LIMIT_HEADER, "50")
                            .with_header(RESET_RATE_LIMIT_HEADER, reset_timestamp.as_str())
                            .with_header(
                                "link",
                                format!(
                                    "<{}{review_url_path}/{REUSED_REVIEW_ID}/comments?page=2>; rel=\"next\"",
                                    server.url()
                                )
                                .as_str(),
                            )
                            .with_body_from_file(asset_path(lib_root, ASSET_REVIEW_COMMENTS_REUSED_PG1).as_str())
                            .create(),
                    );
                    mocks.push(
                        server
                            .mock(
                                "GET",
                                format!("{review_url_path}/{REUSED_REVIEW_ID}/comments").as_str(),
                            )
                            .match_header("Accept", "application/json")
                            .match_header("Authorization", format!("token {TOKEN}").as_str())
                            .match_query(Matcher::UrlEncoded("page".to_string(), "2".to_string()))
                            .with_header(REMAINING_RATE_LIMIT_HEADER, "50")
                            .with_header(RESET_RATE_LIMIT_HEADER, reset_timestamp.as_str())
                            .with_body_from_file(
                                asset_path(lib_root, ASSET_REVIEW_COMMENTS_REUSED_PG2).as_str(),
                            )
                            .create(),
                    );
                    let all_reused_review_comments: Vec<serde_json::Value> =
                        reused_review_comments_pg1
                            .iter()
                            .cloned()
                            .chain(reused_review_comments_pg2.iter().cloned())
                            .collect();
                    test_control_vars.aggregate_review_comments(
                        REUSED_REVIEW_ID,
                        all_reused_review_comments.as_slice(),
                    );
                } else {
                    let all_reused_review_comments =
                        load_json_array(lib_root, ASSET_REVIEW_COMMENTS_REUSED_ALL);
                    mocks.push(
                        server
                            .mock(
                                "GET",
                                format!("{review_url_path}/{REUSED_REVIEW_ID}/comments").as_str(),
                            )
                            .match_header("Accept", "application/json")
                            .match_header("Authorization", format!("token {TOKEN}").as_str())
                            .with_header(REMAINING_RATE_LIMIT_HEADER, "50")
                            .with_header(RESET_RATE_LIMIT_HEADER, reset_timestamp.as_str())
                            .with_body_from_file(
                                asset_path(lib_root, ASSET_REVIEW_COMMENTS_REUSED_ALL).as_str(),
                            )
                            .create(),
                    );
                    test_control_vars.aggregate_review_comments(
                        REUSED_REVIEW_ID,
                        all_reused_review_comments.as_slice(),
                    );
                }

                for comment_id in &test_control_vars.outdated_comment_ids {
                    let fail_resolve_comment =
                        *comment_id == 76453654 && test_params.fail_resolve_comment;
                    mocks.push(
                        server
                            .mock(
                                "POST",
                                format!("/repos/{REPO}/pulls/{PR}/comments/{comment_id}/resolve")
                                    .as_str(),
                            )
                            .match_header("Authorization", format!("token {TOKEN}").as_str())
                            .with_header(REMAINING_RATE_LIMIT_HEADER, "50")
                            .with_header(RESET_RATE_LIMIT_HEADER, reset_timestamp.as_str())
                            .with_status(if fail_resolve_comment { 500 } else { 200 })
                            .create(),
                    );
                }

                for review_id in &test_control_vars.outdated_review_ids {
                    if test_params.delete_review_comments {
                        mocks.push(
                            server
                                .mock("DELETE", format!("{review_url_path}/{review_id}").as_str())
                                .match_header("Authorization", format!("token {TOKEN}").as_str())
                                .with_header(REMAINING_RATE_LIMIT_HEADER, "50")
                                .with_header(RESET_RATE_LIMIT_HEADER, reset_timestamp.as_str())
                                .with_status(if test_params.fail_dismissal { 500 } else { 200 })
                                .create(),
                        );
                    } else {
                        mocks.push(
                            server
                                .mock(
                                    "POST",
                                    format!("{review_url_path}/{review_id}/dismissals").as_str(),
                                )
                                .match_body(Matcher::PartialJson(serde_json::json!({
                                    "message": "Marked as outdated by git-bot-feedback",
                                    "priors": false
                                })))
                                .match_header("Authorization", format!("token {TOKEN}").as_str())
                                .with_header(REMAINING_RATE_LIMIT_HEADER, "50")
                                .with_header(RESET_RATE_LIMIT_HEADER, reset_timestamp.as_str())
                                .with_status(if test_params.fail_dismissal { 500 } else { 200 })
                                .create(),
                        );
                    }
                }
            }
            ExistingReviews::HttpError => {
                mocks.push(
                    server
                        .mock("GET", review_url_path.as_str())
                        .match_header("Accept", "application/json")
                        .match_header("Authorization", format!("token {TOKEN}").as_str())
                        .match_body(Matcher::Any)
                        .match_query(Matcher::UrlEncoded("page".to_string(), "1".to_string()))
                        .with_header(REMAINING_RATE_LIMIT_HEADER, "50")
                        .with_header(RESET_RATE_LIMIT_HEADER, reset_timestamp.as_str())
                        .with_status(403)
                        .with_body("TEST CONDITION TRIGGERED")
                        .create(),
                );
            }
            ExistingReviews::BadJson => {
                mocks.push(
                    server
                        .mock("GET", review_url_path.as_str())
                        .match_header("Accept", "application/json")
                        .match_header("Authorization", format!("token {TOKEN}").as_str())
                        .match_body(Matcher::Any)
                        .match_query(Matcher::UrlEncoded("page".to_string(), "1".to_string()))
                        .with_header(REMAINING_RATE_LIMIT_HEADER, "50")
                        .with_header(RESET_RATE_LIMIT_HEADER, reset_timestamp.as_str())
                        .with_status(200)
                        .with_body("TEST CONDITION TRIGGERED")
                        .create(),
                );
            }
            ExistingReviews::None => unreachable!(),
        }

        let expected_review_body = serde_json::json!({
            "event": "COMMENT",
            "body": format!("{MARKER}{summary}"),
            "commit_id": SHA,
            "comments": [
                {
                    "body": format!("{MARKER}A new comment (without prepended marker)"),
                    "new_position": 42,
                    "old_position": 0,
                    "path": "src/lib.rs",
                },
                {
                    "body": format!("{MARKER}A new comment (with prepended marker)"),
                    "new_position": 42,
                    "old_position": 40,
                    "path": "src/lib.rs",
                },
            ]
        });
        mocks.push(
            server
                .mock("POST", review_url_path.as_str())
                .match_body(Matcher::PartialJson(expected_review_body))
                .match_header("Authorization", format!("token {TOKEN}").as_str())
                .with_header(REMAINING_RATE_LIMIT_HEADER, "50")
                .with_header(RESET_RATE_LIMIT_HEADER, reset_timestamp.as_str())
                .with_status(200)
                .create(),
        );
    }

    let mut opts = ReviewOptions {
        marker: MARKER.to_string(),
        action: ReviewAction::Comment,
        summary,
        comments: test_control_vars.new_review_comments.clone(),
        delete_review_comments: test_params.delete_review_comments,
        ..Default::default()
    };

    client.start_log_group("posting review");
    client.cull_pr_reviews(&mut opts).await.unwrap();
    if let Err(e) = client.post_pr_review(&opts).await {
        if test_params.no_token {
            assert!(matches!(e, RestClientError::EnvVar { .. }));
        } else {
            panic!("Unexpected error posting review: {e}");
        }
    }
    client.end_log_group("");

    for mock in mocks {
        mock.assert();
    }
}

async fn test_reviews(test_params: TestParams) {
    let tmp_dir = TempDir::new().unwrap();
    let lib_root = env::current_dir().unwrap();
    env::set_current_dir(tmp_dir.path()).unwrap();
    setup_and_run(&lib_root, &test_params).await;
    env::set_current_dir(lib_root.as_path()).unwrap();
    drop(tmp_dir);
}

#[tokio::test]
async fn pr() {
    test_reviews(TestParams::default()).await;
}

#[tokio::test]
async fn pr_locked_is_noop() {
    test_reviews(TestParams {
        is_locked: true,
        ..Default::default()
    })
    .await;
}

#[tokio::test]
async fn pr_draft_is_noop() {
    test_reviews(TestParams {
        is_draft: true,
        ..Default::default()
    })
    .await;
}

#[tokio::test]
async fn push_is_noop() {
    test_reviews(TestParams {
        event_t: EventType::Push,
        existing_reviews: ExistingReviews::None,
        ..Default::default()
    })
    .await;
}

#[tokio::test]
async fn no_token_is_error() {
    test_reviews(TestParams {
        no_token: true,
        ..Default::default()
    })
    .await;
}

#[tokio::test]
async fn bad_existing_reviews_are_ignored() {
    test_reviews(TestParams {
        existing_reviews: ExistingReviews::BadJson,
        ..Default::default()
    })
    .await;
}

#[tokio::test]
async fn http_error_get_existing_reviews_are_ignored() {
    test_reviews(TestParams {
        existing_reviews: ExistingReviews::HttpError,
        ..Default::default()
    })
    .await;
}

#[tokio::test]
async fn dismiss_outdated_review_500_is_ignored() {
    test_reviews(TestParams {
        fail_dismissal: true,
        ..Default::default()
    })
    .await;
}

#[tokio::test]
async fn delete_outdated_review_comments() {
    test_reviews(TestParams {
        delete_review_comments: true,
        ..Default::default()
    })
    .await;
}

#[tokio::test]
async fn delete_outdated_review_comment_500_is_ignored() {
    test_reviews(TestParams {
        delete_review_comments: true,
        fail_resolve_comment: true,
        ..Default::default()
    })
    .await;
}

#[tokio::test]
async fn review_comments_500_is_ignored() {
    test_reviews(TestParams {
        fail_get_review_comments: true,
        ..Default::default()
    })
    .await;
}

#[tokio::test]
async fn review_with_zero_comment_count() {
    test_reviews(TestParams {
        review_with_no_comments: true,
        ..Default::default()
    })
    .await;
}

#[tokio::test]
async fn paginated_review_comments() {
    test_reviews(TestParams {
        paginate_review_comments: true,
        ..Default::default()
    })
    .await;
}
