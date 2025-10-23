use chrono::Utc;
use git_bot_feedback::{
    CommentKind, CommentPolicy, RestApiClient, RestClientError, ThreadCommentOptions,
    client::GithubApiClient,
};
use mockito::{Matcher, Server};
use std::{env, fmt::Display, io::Write, path::Path};
use tempfile::{NamedTempFile, TempDir};

mod common;
use common::logger_init;

const MARKER: &str = "<!-- git-bot-feedback -->\n";
const SHA: &str = "deadbeef";
const REPO: &str = "2bndy5/git-bot-feedback";
const PR: i64 = 22;
const TOKEN: &str = "123456";
const MOCK_ASSETS_PATH: &str = "tests/assets/thread_comment/github/";
const EVENT_PAYLOAD: &str = "{\"number\": 22}";

const RESET_RATE_LIMIT_HEADER: &str = "x-ratelimit-reset";
const REMAINING_RATE_LIMIT_HEADER: &str = "x-ratelimit-remaining";

#[derive(PartialEq, Clone, Copy, Debug)]
enum EventType {
    Push,
    PullRequest,
}

impl Display for EventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Push => write!(f, "push"),
            Self::PullRequest => write!(f, "pull_request"),
        }
    }
}

struct TestParams {
    event_t: EventType,
    comment_policy: CommentPolicy,
    no_lgtm: bool,
    comment_kind: CommentKind,
    fail_get_existing_comments: bool,
    fail_dismissal: bool,
    fail_posting: bool,
    bad_existing_comments: bool,
    bad_pr_info: bool,
    no_token: bool,
}

impl Default for TestParams {
    fn default() -> Self {
        Self {
            event_t: EventType::Push,
            comment_policy: CommentPolicy::Update,
            no_lgtm: false,
            comment_kind: CommentKind::Concerns,
            fail_get_existing_comments: false,
            fail_dismissal: false,
            fail_posting: false,
            bad_existing_comments: false,
            bad_pr_info: false,
            no_token: false,
        }
    }
}

async fn setup(lib_root: &Path, test_params: &TestParams) {
    unsafe {
        env::set_var(
            "GITHUB_EVENT_NAME",
            test_params.event_t.to_string().as_str(),
        );
        env::set_var("GITHUB_REPOSITORY", REPO);
        env::set_var("GITHUB_SHA", SHA);
        if !test_params.no_token {
            env::set_var("GITHUB_TOKEN", TOKEN);
        }
        env::set_var("CI", "true");
        if env::var("ACTIONS_STEP_DEBUG").is_err() {
            env::set_var("ACTIONS_STEP_DEBUG", "true");
        }
    }
    let mut event_payload_path = NamedTempFile::new_in("./").unwrap();
    if test_params.event_t == EventType::PullRequest {
        event_payload_path
            .write_all(if test_params.bad_pr_info {
                "EVENT_PAYLOAD".as_bytes()
            } else {
                EVENT_PAYLOAD.as_bytes()
            })
            .expect("Failed to create mock event payload.");
        unsafe {
            env::set_var("GITHUB_EVENT_PATH", event_payload_path.path());
        }
    }

    let reset_timestamp = (Utc::now().timestamp() + 60).to_string();
    let asset_path = format!("{}/{MOCK_ASSETS_PATH}", lib_root.to_str().unwrap());

    let mut server = Server::new_async().await;
    unsafe {
        env::set_var("GITHUB_API_URL", server.url());
    }

    logger_init();
    log::set_max_level(log::LevelFilter::Debug);
    let client = match GithubApiClient::new() {
        Ok(c) => c,
        Err(e) => {
            assert!(test_params.bad_pr_info);
            assert!(matches!(e, RestClientError::JsonError(_)));
            return;
        }
    };
    assert!(client.debug_enabled);

    let mut mocks = vec![];

    if test_params.event_t == EventType::Push {
        let mut mock = server
            .mock(
                "GET",
                format!("/repos/{REPO}/commits/{SHA}/comments").as_str(),
            )
            .match_header("Accept", "application/vnd.github.raw+json")
            .match_body(Matcher::Any)
            .match_query(Matcher::UrlEncoded("page".to_string(), "1".to_string()))
            .with_header(REMAINING_RATE_LIMIT_HEADER, "50")
            .with_header(RESET_RATE_LIMIT_HEADER, reset_timestamp.as_str())
            .with_status(
                if test_params.fail_get_existing_comments || test_params.no_token {
                    403
                } else {
                    200
                },
            );
        if !test_params.no_token {
            mock = mock.match_header("Authorization", format!("token {TOKEN}").as_str());
        }
        if test_params.bad_existing_comments || test_params.no_token {
            mock = mock.with_body(String::new());
        } else {
            eprintln!("{asset_path}push_comments_{SHA}.json");
            mock = mock.with_body_from_file(format!("{asset_path}push_comments_{SHA}.json"));
        }
        mock = mock.create();
        mocks.push(mock);
    } else {
        let pr_endpoint = format!("/repos/{REPO}/issues/{PR}/comments");
        for pg in ["1", "2"] {
            let link = if pg == "1" {
                format!("<{}{pr_endpoint}?page=2>; rel=\"next\"", server.url())
            } else {
                "".to_string()
            };
            mocks.push(
                server
                    .mock("GET", pr_endpoint.as_str())
                    .match_header("Accept", "application/vnd.github.raw+json")
                    .match_header("Authorization", format!("token {TOKEN}").as_str())
                    .match_body(Matcher::Any)
                    .match_query(Matcher::UrlEncoded("page".to_string(), pg.to_string()))
                    .with_body_from_file(format!("{asset_path}pr_comments_pg{pg}.json"))
                    .with_header(REMAINING_RATE_LIMIT_HEADER, "50")
                    .with_header(RESET_RATE_LIMIT_HEADER, reset_timestamp.as_str())
                    .with_header("link", link.as_str())
                    .with_status(if test_params.fail_dismissal { 403 } else { 200 })
                    .create(),
            );
        }
    }
    let comment_url = format!(
        "/repos/{REPO}{}/comments/76453652",
        if test_params.event_t == EventType::PullRequest {
            "/issues"
        } else {
            ""
        }
    );

    if !test_params.fail_get_existing_comments
        && !test_params.bad_existing_comments
        && !test_params.no_token
    {
        mocks.push(
            server
                .mock("DELETE", comment_url.as_str())
                .match_body(Matcher::Any)
                .match_header("Authorization", format!("token {TOKEN}").as_str())
                .with_status(if test_params.fail_dismissal { 403 } else { 200 })
                .with_header(REMAINING_RATE_LIMIT_HEADER, "50")
                .with_header(RESET_RATE_LIMIT_HEADER, reset_timestamp.as_str())
                .expect_at_least(1)
                .create(),
        );
    }

    let comment = match test_params.comment_kind {
        CommentKind::Concerns => "Attention".to_string(),
        CommentKind::Lgtm => "LGTM".to_string(),
    };
    let new_comment_match = Matcher::Regex(comment.clone());

    let posting_comment = match test_params.comment_kind {
        CommentKind::Concerns => true,
        CommentKind::Lgtm => !test_params.no_lgtm,
    };
    if posting_comment {
        if test_params.bad_existing_comments
            || test_params.fail_get_existing_comments
            || test_params.comment_policy == CommentPolicy::Anew
            || test_params.no_token
        {
            let mut mock = server
                .mock(
                    "POST",
                    format!(
                        "/repos/{REPO}/{}/comments",
                        if test_params.event_t == EventType::PullRequest {
                            format!("issues/{PR}")
                        } else {
                            format!("commits/{SHA}")
                        }
                    )
                    .as_str(),
                )
                .match_body(new_comment_match)
                .with_header(REMAINING_RATE_LIMIT_HEADER, "50")
                .with_header(RESET_RATE_LIMIT_HEADER, reset_timestamp.as_str())
                .with_status(if test_params.fail_posting { 403 } else { 200 })
                .create();
            if !test_params.no_token {
                mock = mock.match_header("Authorization", format!("token {TOKEN}").as_str());
            }
            mocks.push(mock);
        } else {
            mocks.push(
                server
                    .mock("PATCH", comment_url.as_str())
                    .match_body(new_comment_match.clone())
                    .match_header("Authorization", format!("token {TOKEN}").as_str())
                    .with_status(if test_params.fail_posting { 403 } else { 200 })
                    .with_header(REMAINING_RATE_LIMIT_HEADER, "50")
                    .with_header(RESET_RATE_LIMIT_HEADER, reset_timestamp.as_str())
                    .create(),
            );
        }
    }

    let opts = ThreadCommentOptions {
        policy: test_params.comment_policy,
        comment,
        kind: test_params.comment_kind,
        marker: MARKER.to_string(),
        no_lgtm: test_params.no_lgtm,
    };
    GithubApiClient::start_log_group("posting comment");
    let result = client.post_thread_comment(opts).await;
    GithubApiClient::end_log_group();
    assert!(result.is_ok());
    for mock in mocks {
        mock.assert();
    }
}

async fn test_comment(test_params: &TestParams) {
    let tmp_dir = TempDir::new().unwrap();
    let lib_root = env::current_dir().unwrap();
    env::set_current_dir(tmp_dir.path()).unwrap();
    setup(&lib_root, test_params).await;
    env::set_current_dir(lib_root.as_path()).unwrap();
    drop(tmp_dir);
}

#[tokio::test]
async fn new_push() {
    test_comment(&TestParams {
        comment_policy: CommentPolicy::Anew,
        ..Default::default()
    })
    .await;
}

#[tokio::test]
async fn new_pr() {
    test_comment(&TestParams {
        event_t: EventType::PullRequest,
        comment_policy: CommentPolicy::Anew,
        ..Default::default()
    })
    .await;
}

#[tokio::test]
async fn update_push() {
    test_comment(&TestParams::default()).await;
}

#[tokio::test]
async fn update_pr() {
    test_comment(&TestParams {
        event_t: EventType::PullRequest,
        ..Default::default()
    })
    .await;
}

#[tokio::test]
async fn new_push_no_lgtm() {
    test_comment(&TestParams {
        comment_policy: CommentPolicy::Anew,
        comment_kind: CommentKind::Lgtm,
        no_lgtm: true,
        ..Default::default()
    })
    .await;
}

#[tokio::test]
async fn update_push_no_lgtm() {
    test_comment(&TestParams {
        comment_kind: CommentKind::Lgtm,
        no_lgtm: true,
        ..Default::default()
    })
    .await;
}

#[tokio::test]
async fn new_pr_no_lgtm() {
    test_comment(&TestParams {
        comment_policy: CommentPolicy::Anew,
        event_t: EventType::PullRequest,
        no_lgtm: true,
        comment_kind: CommentKind::Lgtm,
        ..Default::default()
    })
    .await;
}

#[tokio::test]
async fn update_pr_no_lgtm() {
    test_comment(&TestParams {
        event_t: EventType::PullRequest,
        comment_kind: CommentKind::Lgtm,
        no_lgtm: true,
        ..Default::default()
    })
    .await;
}

#[tokio::test]
async fn fail_get_existing_comments() {
    test_comment(&TestParams {
        fail_get_existing_comments: true,
        ..Default::default()
    })
    .await;
}

#[tokio::test]
async fn fail_dismissal() {
    test_comment(&TestParams {
        fail_dismissal: true,
        ..Default::default()
    })
    .await;
}

#[tokio::test]
async fn fail_posting() {
    test_comment(&TestParams {
        fail_posting: true,
        ..Default::default()
    })
    .await;
}

#[tokio::test]
async fn bad_existing_comments() {
    test_comment(&TestParams {
        bad_existing_comments: true,
        comment_kind: CommentKind::Lgtm,
        ..Default::default()
    })
    .await;
}

#[tokio::test]
async fn bad_pr_info() {
    test_comment(&TestParams {
        event_t: EventType::PullRequest,
        bad_pr_info: true,
        ..Default::default()
    })
    .await;
}

#[tokio::test]
async fn no_token() {
    test_comment(&TestParams {
        no_token: true,
        ..Default::default()
    })
    .await;
}
