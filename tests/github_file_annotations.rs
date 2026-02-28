use git_bot_feedback::{FileAnnotation, RestApiClient, client::GithubApiClient};
use mockito::Server;
use std::env;
mod common;
use common::logger_init;

#[derive(Debug, Default)]
struct TestParams {
    empty_array: bool,
}

const REPO: &str = "2bndy5/git-bot-feedback";
const SHA: &str = "DEADBEEF";

async fn write_annotations(test_params: TestParams) {
    unsafe {
        env::set_var("GITHUB_REPOSITORY", REPO);
        env::set_var("GITHUB_SHA", SHA);
        env::set_var("CI", "true");
        env::set_var("GITHUB_EVENT_NAME", "push");
    };
    let server = Server::new_async().await;
    unsafe {
        env::set_var("GITHUB_API_URL", server.url());
    }

    logger_init();
    log::set_max_level(log::LevelFilter::Debug);
    let client = GithubApiClient::new().unwrap();

    let annotations = if test_params.empty_array {
        vec![]
    } else {
        vec![FileAnnotation {
            message: "Test annotation".to_string(),
            ..Default::default()
        }]
    };
    client.write_file_annotations(&annotations).unwrap();
}

#[tokio::test]
async fn some_annotations() {
    write_annotations(TestParams::default()).await;
}

#[tokio::test]
async fn empty_annotations() {
    write_annotations(TestParams {
        empty_array: true,
        ..Default::default()
    })
    .await;
}
