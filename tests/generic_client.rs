use chrono::Utc;
use git_bot_feedback::{
    RestApiClient, RestApiRateLimitHeaders, RestClientError, ThreadCommentOptions,
    client::send_api_request,
};
use mockito::{Matcher, Server};
use reqwest::{
    Client, Method, StatusCode,
    header::{HeaderMap, HeaderName, HeaderValue},
};
mod common;
use common::logger_init;

/// A dummy struct to impl RestApiClient
#[derive(Default)]
struct TestClient;

impl RestApiClient for TestClient {
    fn make_headers() -> Result<HeaderMap<HeaderValue>, RestClientError> {
        let map = HeaderMap::new();
        HeaderValue::from_str("\0")
            .map(|_| map)
            .map_err(RestClientError::InvalidHeaderValue)
    }

    async fn post_thread_comment(
        &self,
        _options: ThreadCommentOptions,
    ) -> Result<(), RestClientError> {
        Err(RestClientError::RequestCloneError)
    }

    fn start_log_group(name: &str) {
        log::info!(target: "CI_LOG_GROUPING", "start_log_group: {name}");
    }

    fn end_log_group() {
        log::info!(target: "CI_LOG_GROUPING", "end_log_group");
    }

    fn is_pr_event(&self) -> bool {
        false
    }

    fn write_output_variables(
        _vars: &[git_bot_feedback::OutputVariable],
    ) -> Result<(), RestClientError> {
        Err(RestClientError::Io(std::io::Error::from(
            std::io::ErrorKind::InvalidFilename,
        )))
    }
}

#[derive(Default)]
struct RateLimitTestParams {
    secondary: bool,
    has_remaining_count: bool,
    bad_remaining_count: bool,
    has_reset_timestamp: bool,
    bad_reset_timestamp: bool,
    has_retry_interval: bool,
    bad_retry_interval: bool,
}

async fn simulate_rate_limit(test_params: &RateLimitTestParams) {
    let rate_limit_headers = RestApiRateLimitHeaders {
        reset: "reset".to_string(),
        remaining: "remaining".to_string(),
        retry: "retry".to_string(),
    };
    logger_init();
    log::set_max_level(log::LevelFilter::Debug);

    let mut server = Server::new_async().await;
    let client = Client::new();
    let reset_timestamp = (Utc::now().timestamp() + 60).to_string();
    let mut mock = server
        .mock("GET", "/")
        .match_body(Matcher::Any)
        .expect_at_least(1)
        .expect_at_most(5)
        .with_status(429);
    if test_params.has_remaining_count {
        mock = mock.with_header(
            &rate_limit_headers.remaining,
            if test_params.secondary {
                "1"
            } else if test_params.bad_remaining_count {
                "X"
            } else {
                "0"
            },
        );
    }
    if test_params.has_reset_timestamp {
        mock = mock.with_header(
            &rate_limit_headers.reset,
            if test_params.bad_reset_timestamp {
                "X"
            } else {
                &reset_timestamp
            },
        );
    }
    if test_params.secondary && test_params.has_retry_interval {
        mock.with_header(
            &rate_limit_headers.retry,
            if test_params.bad_retry_interval {
                "X"
            } else {
                "0"
            },
        )
        .create();
    } else {
        mock.create();
    }
    let request =
        TestClient::make_api_request(&client, server.url(), Method::GET, None, None).unwrap();
    let result = send_api_request(&client, request, &rate_limit_headers).await;
    let err = match result {
        Ok(response) => {
            let result = response.error_for_status();
            result.map_err(RestClientError::Request).unwrap_err()
        }
        Err(e) => e,
    };
    if let RestClientError::Request(e) = err {
        assert!(matches!(e.status(), Some(StatusCode::TOO_MANY_REQUESTS)));
    } else {
        assert!(matches!(err, RestClientError::RateLimit));
    }
}

#[tokio::test]
async fn rate_limit_secondary() {
    simulate_rate_limit(&RateLimitTestParams {
        secondary: true,
        has_retry_interval: true,
        ..Default::default()
    })
    .await;
}

#[tokio::test]
async fn rate_limit_bad_retry() {
    simulate_rate_limit(&RateLimitTestParams {
        secondary: true,
        has_retry_interval: true,
        bad_retry_interval: true,
        ..Default::default()
    })
    .await;
}

#[tokio::test]
async fn rate_limit_primary() {
    simulate_rate_limit(&RateLimitTestParams {
        has_remaining_count: true,
        has_reset_timestamp: true,
        ..Default::default()
    })
    .await;
}

#[tokio::test]
async fn rate_limit_no_reset() {
    simulate_rate_limit(&RateLimitTestParams {
        has_remaining_count: true,
        ..Default::default()
    })
    .await;
}

#[tokio::test]
async fn rate_limit_bad_reset() {
    simulate_rate_limit(&RateLimitTestParams {
        has_remaining_count: true,
        has_reset_timestamp: true,
        bad_reset_timestamp: true,
        ..Default::default()
    })
    .await;
}

#[tokio::test]
async fn rate_limit_bad_count() {
    simulate_rate_limit(&RateLimitTestParams {
        has_remaining_count: true,
        bad_remaining_count: true,
        ..Default::default()
    })
    .await;
}

#[tokio::test]
async fn dummy_coverage() {
    assert!(TestClient::make_headers().is_err());
    let dummy = TestClient;
    TestClient::start_log_group("Dummy test");
    assert!(
        dummy
            .post_thread_comment(ThreadCommentOptions {
                comment: "some comment text".to_string(),
                ..Default::default()
            })
            .await
            .is_err()
    );
    TestClient::append_step_summary("").unwrap();
    TestClient::write_output_variables(&[]).expect_err("Not implemented for generic clients");
    assert!(!dummy.is_pr_event());
    TestClient::end_log_group();
}

// ************************************************* try_next_page() tests

#[test]
fn bad_link_header() {
    let mut headers = HeaderMap::with_capacity(1);
    assert!(
        headers
            .insert("link", HeaderValue::from_str("; rel=\"next\"").unwrap())
            .is_none()
    );
    logger_init();
    log::set_max_level(log::LevelFilter::Debug);
    let result = TestClient::try_next_page(&headers);
    assert!(result.is_none());
}

#[test]
fn bad_link_domain() {
    let mut headers = HeaderMap::with_capacity(1);
    assert!(
        headers
            .insert(
                "link",
                HeaderValue::from_str("<not a domain>; rel=\"next\"").unwrap()
            )
            .is_none()
    );
    logger_init();
    log::set_max_level(log::LevelFilter::Debug);
    let result = TestClient::try_next_page(&headers);
    assert!(result.is_none());
}

#[test]
fn mk_request() {
    let client = Client::new();
    let url = "https://127.0.0.1";
    let method = Method::GET;
    let data = "text".to_string();
    let header_value = HeaderValue::from_str("value").unwrap();
    let headers = Some(HeaderMap::from_iter([(
        HeaderName::from_static("key"),
        header_value.clone(),
    )]));
    let request =
        TestClient::make_api_request(&client, url, method, Some(data.clone()), headers.clone())
            .unwrap();
    assert_eq!(request.body().unwrap().as_bytes(), Some(data.as_bytes()));
    assert!(
        request
            .headers()
            .get("key")
            .is_some_and(|v| *v == header_value)
    );
}

/// uses a relative url to trigger a reqwest::RequestBuilder error.
#[test]
fn bad_request() {
    let client = Client::new();
    let result = TestClient::make_api_request(&client, "127.0.0.1", Method::GET, None, None);
    eprintln!("err: {result:?}");
    assert!(result.is_err_and(|e| matches!(e, RestClientError::Request(_))));
}

#[tokio::test]
#[cfg(feature = "file-changes")]
async fn list_file_changes() {
    use std::{env, fs::OpenOptions, io::Write, process::Command};

    use common::logger_init;
    use git_bot_feedback::{FileFilter, LinesChangedOnly};
    use tempfile::TempDir;

    // Setup temp workspace
    let tmp_dir = TempDir::new().unwrap();
    Command::new("git")
        .current_dir(tmp_dir.path())
        .args([
            "clone",
            "--depth=2",       // only checkout HEAD and its parent commit (HEAD~1)
            "--branch=v0.1.4", // https://github.com/2bndy5/git-bot-feedback/commit/19c6330e8c4aa0e4ee18482b761277bd294bb6f3
            "https://github.com/2bndy5/git-bot-feedback.git",
        ])
        .output()
        .unwrap();
    env::set_current_dir(tmp_dir.path().join("git-bot-feedback")).unwrap();

    // setup test client, logging, and file filter
    logger_init();
    log::set_max_level(log::LevelFilter::Debug);
    let client = TestClient::default();
    let file_filter = FileFilter::new(&[], &["toml", "md"], None);

    // Now get diff of HEAD and parent commit
    let changes = client
        .get_list_of_changed_files(&file_filter, &LinesChangedOnly::On, &None::<u8>, false)
        .await
        .unwrap();
    assert_eq!(changes.len(), 2);
    let expected_changed_file = String::from("Cargo.toml");
    assert!(changes.contains_key(&expected_changed_file));
    assert_eq!(
        changes.get(&expected_changed_file).unwrap().added_lines,
        vec![4] // line 4 is where the version is defined in Cargo.toml
    );

    // make uncommitted change and verify it is detected
    let mut cargo_toml = OpenOptions::new()
        .append(true)
        .open(&expected_changed_file)
        .unwrap();
    cargo_toml.write_all(b"# Dummy change").unwrap();
    cargo_toml.sync_all().unwrap();
    Command::new("git")
        .args(["add", &expected_changed_file])
        .output()
        .unwrap();

    // Get diff of working directory
    let changes = client
        .get_list_of_changed_files(&file_filter, &LinesChangedOnly::On, &None::<u8>, false)
        .await
        .unwrap();
    // only the staged change should be detected, not the uncommitted change
    assert_eq!(changes.len(), 1);
    assert!(changes.contains_key(&expected_changed_file));
    let added_lines = &changes.get(&expected_changed_file).unwrap().added_lines;
    assert_eq!(added_lines.len(), 1);
    // The added line should not be line 4 anymore.
    // It should be the new line we just added at the end of the file.
    assert_ne!(*added_lines.first().unwrap(), 4);

    // test custom diff base provided as an invalid git ref
    assert!(matches!(
        client
            .get_list_of_changed_files(&file_filter, &LinesChangedOnly::Diff, &Some("1.0"), true)
            .await
            .unwrap_err(),
        RestClientError::GitCommandError(_)
    ));

    // test custom diff base provided as a number of parents from HEAD
    let changes = client
        .get_list_of_changed_files(&file_filter, &LinesChangedOnly::On, &Some(1), true)
        .await
        .unwrap();
    assert!(changes.contains_key(&expected_changed_file));
    assert_eq!(changes.len(), 2);

    // test custom diff base provided as a valid git ref
    let valid_ref = {
        let git_out = Command::new("git")
            .args(["rev-parse", "HEAD~1"])
            .output()
            .unwrap()
            .stdout;
        String::from_utf8_lossy(&git_out).trim().to_string()
    };
    let changes = client
        .get_list_of_changed_files(&file_filter, &LinesChangedOnly::On, &Some(valid_ref), true)
        .await
        .unwrap();
    assert!(changes.contains_key(&expected_changed_file));
    assert_eq!(changes.len(), 2);
}
