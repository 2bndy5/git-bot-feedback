use std::env;

use chrono::Utc;
use git_bot_feedback::{
    FileAnnotation, OutputVariable, RestApiClient, RestApiRateLimitHeaders, RestClientError,
    ReviewOptions, ThreadCommentOptions,
    client::{LocalClient, init_client},
};
use mockito::{Matcher, Server};
use reqwest::{
    Client, Method, StatusCode,
    header::{HeaderMap, HeaderName, HeaderValue},
};
use url::Url;

mod common;
use common::logger_init;

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
    unsafe {
        // prevent using git-server clients in these tests
        env::remove_var("GITHUB_ACTIONS");
        env::remove_var("GITEA_ACTIONS");
    }
    let test_client = init_client().unwrap();
    assert!(!test_client.is_debug_enabled());
    assert!(test_client.event_name().is_none());
    let request = test_client
        .make_api_request(
            &client,
            Url::parse(&server.url()).unwrap(),
            Method::GET,
            None,
            None,
        )
        .unwrap();
    let result = test_client
        .send_api_request(&client, request, &rate_limit_headers)
        .await;
    let err = match result {
        Ok(response) => {
            let result = response.error_for_status();
            result.map_err(RestClientError::Request).unwrap_err()
        }
        Err(e) => e,
    };
    if let RestClientError::Request(e) = err {
        assert!(matches!(e.status(), Some(StatusCode::TOO_MANY_REQUESTS)));
    } else if test_params.bad_retry_interval
        || test_params.bad_remaining_count
        || test_params.bad_reset_timestamp
    {
        assert!(matches!(err, RestClientError::HeaderParseInt(_)));
    } else if test_params.has_reset_timestamp {
        assert!(matches!(err, RestClientError::RateLimitPrimary(_)));
    } else if test_params.secondary {
        assert!(matches!(err, RestClientError::RateLimitSecondary));
    } else {
        assert!(matches!(err, RestClientError::RateLimitNoReset));
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
    let mut test_client = LocalClient;
    let log_group_name = "Dummy test";
    test_client.start_log_group(log_group_name);
    test_client
        .post_thread_comment(ThreadCommentOptions {
            comment: "some comment text".to_string(),
            ..Default::default()
        })
        .await
        .unwrap();
    test_client.append_step_summary("").unwrap();
    test_client
        .write_output_variables(&[OutputVariable {
            name: "key".to_string(),
            value: "value".to_string(),
        }])
        .unwrap();
    assert!(!test_client.is_pr_event());
    test_client.set_user_agent("user_agent").unwrap();
    let mut options = ReviewOptions::default();
    test_client.cull_pr_reviews(&mut options).await.unwrap();
    test_client.post_pr_review(&options).await.unwrap();
    let annotation = FileAnnotation::default();
    test_client.write_file_annotations(&[annotation]).unwrap();
    test_client.end_log_group(log_group_name);
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
    let test_client = LocalClient;
    let result = test_client.try_next_page(&headers);
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
    let test_client = LocalClient;
    let result = test_client.try_next_page(&headers);
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
    let test_client = LocalClient;
    let request = test_client
        .make_api_request(
            &client,
            Url::parse(url).unwrap(),
            method,
            Some(data.clone()),
            headers.clone(),
        )
        .unwrap();
    assert_eq!(request.body().unwrap().as_bytes(), Some(data.as_bytes()));
    assert!(
        request
            .headers()
            .get("key")
            .is_some_and(|v| *v == header_value)
    );
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
    let client = LocalClient;
    assert_eq!(client.client_kind().as_str(), "local");
    let file_filter = FileFilter::new(&[], &["toml", "md"], None);

    // Now get diff of HEAD and parent commit
    let changes = client
        .get_list_of_changed_files(&file_filter, &LinesChangedOnly::On, None, false)
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
        .get_list_of_changed_files(&file_filter, &LinesChangedOnly::On, None, false)
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
            .get_list_of_changed_files(
                &file_filter,
                &LinesChangedOnly::Diff,
                Some("1.0".to_string()),
                true
            )
            .await
            .unwrap_err(),
        RestClientError::GitCommand(_)
    ));

    // test custom diff base provided as a number of parents from HEAD
    let changes = client
        .get_list_of_changed_files(
            &file_filter,
            &LinesChangedOnly::On,
            Some(0.to_string()),
            true,
        )
        .await
        .unwrap();
    assert!(changes.contains_key(&expected_changed_file));
    assert_eq!(changes.len(), 2);

    // test custom diff base provided as a valid git ref
    let valid_ref = {
        let git_out = Command::new("git")
            .args(["rev-parse", "HEAD~0"])
            .output()
            .unwrap()
            .stdout;
        String::from_utf8_lossy(&git_out).trim().to_string()
    };
    let changes = client
        .get_list_of_changed_files(&file_filter, &LinesChangedOnly::On, Some(valid_ref), true)
        .await
        .unwrap();
    assert!(changes.contains_key(&expected_changed_file));
    assert_eq!(changes.len(), 2);
}

#[cfg(feature = "file-changes")]
#[test]
fn walk_dir_strip_prefix() {
    use git_bot_feedback::FileFilter;

    let extensions = vec!["json"];
    logger_init();
    log::set_max_level(log::LevelFilter::Debug);
    let file_filter = FileFilter::new(&["", "!tests"], &extensions, Some("abs-path"));
    let abs_path = std::env::current_dir().unwrap();
    println!("current dir: {}", abs_path.to_string_lossy());
    let files = file_filter.walk_dir(&abs_path).unwrap();
    println!("discovered files: {:?}", files);
    assert!(files.contains("tests/assets/file_changes/github/pr_files_pg1.json"));
}

/// Run a git command in `dir` with a throwaway identity, and assert it succeeds.
#[cfg(feature = "file-changes")]
fn git(dir: &std::path::Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .current_dir(dir)
        .args(["-c", "user.name=test", "-c", "user.email=test@example.com"])
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// An untracked file must not hide the unstaged changes, and is itself reported as a new file.
#[tokio::test]
#[cfg(feature = "file-changes")]
async fn list_untracked_files() {
    use git_bot_feedback::{FileFilter, LinesChangedOnly};
    use tempfile::TempDir;

    logger_init();
    log::set_max_level(log::LevelFilter::Debug);
    let tmp_dir = TempDir::new().unwrap();
    let repo = tmp_dir.path();
    git(repo, &["init", "--quiet"]);
    std::fs::create_dir(repo.join("src")).unwrap();
    std::fs::write(repo.join("src/tracked.cpp"), "int a;\n").unwrap();
    std::fs::write(repo.join(".gitignore"), "build/\n").unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "--quiet", "-m", "init"]);

    // An unstaged change to a tracked file.
    std::fs::write(repo.join("src/tracked.cpp"), "int a;\nint b;\n").unwrap();
    // A new file that is not staged.
    std::fs::write(repo.join("src/new.cpp"), "int c;\nint d;\n").unwrap();
    // Untracked files that the filter or .gitignore leave out.
    std::fs::write(repo.join("results.sarif"), "{}\n").unwrap();
    std::fs::create_dir(repo.join("build")).unwrap();
    std::fs::write(repo.join("build/gen.cpp"), "int e;\n").unwrap();

    // Run from a subdirectory: paths must still be relative to the repository root.
    env::set_current_dir(repo.join("src")).unwrap();
    let client = LocalClient;
    let file_filter = FileFilter::new(&[], &["cpp"], None);
    let changes = client
        .get_list_of_changed_files(
            &file_filter,
            &LinesChangedOnly::On,
            Some("HEAD".to_string()),
            false,
        )
        .await
        .unwrap();
    let mut names = changes.keys().cloned().collect::<Vec<_>>();
    names.sort();
    assert_eq!(names, vec!["src/new.cpp", "src/tracked.cpp"]);
    assert_eq!(changes["src/tracked.cpp"].added_lines, vec![2]);
    assert_eq!(changes["src/new.cpp"].added_lines, vec![1, 2]);

    // Once something is staged, only the staged changes count (as pre-commit expects),
    // so the untracked file is left out again.
    git(repo, &["add", "src/tracked.cpp"]);
    let changes = client
        .get_list_of_changed_files(
            &file_filter,
            &LinesChangedOnly::On,
            Some("HEAD".to_string()),
            false,
        )
        .await
        .unwrap();
    assert_eq!(changes.keys().collect::<Vec<_>>(), vec!["src/tracked.cpp"]);
}

/// Without a diff base, a repository with only one commit (or none) must not fail on `HEAD~1`.
#[tokio::test]
#[cfg(feature = "file-changes")]
async fn list_changes_first_commit() {
    use git_bot_feedback::{FileFilter, LinesChangedOnly};
    use tempfile::TempDir;

    logger_init();
    log::set_max_level(log::LevelFilter::Debug);
    let tmp_dir = TempDir::new().unwrap();
    let repo = tmp_dir.path();
    git(repo, &["init", "--quiet"]);
    env::set_current_dir(repo).unwrap();
    let client = LocalClient;
    let file_filter = FileFilter::new(&[], &["cpp"], None);
    let list = async |ignore_index: bool| {
        let changes = client
            .get_list_of_changed_files(&file_filter, &LinesChangedOnly::On, None, ignore_index)
            .await
            .unwrap();
        let mut names = changes
            .iter()
            .map(|(name, lines)| (name.clone(), lines.added_lines.clone()))
            .collect::<Vec<_>>();
        names.sort();
        names
    };

    // No commit yet: new files are untracked.
    std::fs::write(repo.join("a.cpp"), "int a;\n").unwrap();
    assert_eq!(list(false).await, vec![("a.cpp".to_string(), vec![1])]);

    // Only one commit: its files count as new, together with the working tree changes.
    git(repo, &["add", "."]);
    git(repo, &["commit", "--quiet", "-m", "init"]);
    std::fs::write(repo.join("a.cpp"), "int a;\nint b;\n").unwrap();
    assert_eq!(list(false).await, vec![("a.cpp".to_string(), vec![1, 2])]);

    // With `ignore_index`, the first commit is shown as it is.
    assert_eq!(list(true).await, vec![("a.cpp".to_string(), vec![1])]);
}

#[test]
fn local_annotations_one_line() {
    use git_bot_feedback::AnnotationLevel;

    let file_level = FileAnnotation {
        severity: AnnotationLevel::Notice,
        path: "./src\\demo.cpp".to_string(),
        title: Some("clang-format".to_string()),
        message: "File src/demo.cpp does not conform to LLVM style guidelines. (lines 2,6)"
            .to_string(),
        ..Default::default()
    };
    assert_eq!(
        file_level.fmt_local(),
        "src/demo.cpp: note: File src/demo.cpp does not conform to LLVM style guidelines. (lines 2,6) [clang-format]"
    );
    let line_only = FileAnnotation {
        severity: AnnotationLevel::Error,
        path: "src/demo.cpp".to_string(),
        start_line: Some(0),
        start_column: None,
        message: "line 0 is shown as line 1".to_string(),
        ..Default::default()
    };
    assert_eq!(
        line_only.fmt_local(),
        "src/demo.cpp:1: error: line 0 is shown as line 1"
    );
    let no_path = FileAnnotation {
        severity: AnnotationLevel::Debug,
        start_line: Some(3),
        message: "a\r\nb".to_string(),
        ..Default::default()
    };
    assert_eq!(no_path.fmt_local(), "debug: a\nb");
    LocalClient
        .write_file_annotations(&[file_level, line_only, no_path])
        .unwrap();
}
