#![allow(clippy::unwrap_used)]

use git_bot_feedback::client::same_origin_redirect_policy;
use mockito::Server;
use reqwest::Client;

fn client() -> Client {
    Client::builder()
        .redirect(same_origin_redirect_policy())
        .build()
        .unwrap()
}

#[tokio::test]
async fn follows_same_origin_redirect() {
    let mut server = Server::new_async().await;
    let redirect = server
        .mock("GET", "/start")
        .with_status(302)
        .with_header("location", "/final")
        .create_async()
        .await;
    let target = server
        .mock("GET", "/final")
        .match_header("private-token", "secret")
        .with_body("ok")
        .create_async()
        .await;

    let response = client()
        .get(format!("{}/start", server.url()))
        .header("PRIVATE-TOKEN", "secret")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.text().await.unwrap(), "ok");
    redirect.assert_async().await;
    target.assert_async().await;
}

#[tokio::test]
async fn stops_cross_origin_redirect() {
    let mut origin = Server::new_async().await;
    let mut other = Server::new_async().await;
    // different ports mean different origins
    let location = format!("{}/final", other.url());
    let redirect = origin
        .mock("GET", "/start")
        .with_status(302)
        .with_header("location", &location)
        .create_async()
        .await;
    let target = other.mock("GET", "/final").expect(0).create_async().await;

    let response = client()
        .get(format!("{}/start", origin.url()))
        .header("PRIVATE-TOKEN", "secret")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 302);
    assert_eq!(response.headers()["location"], location.as_str());
    redirect.assert_async().await;
    target.assert_async().await;
}

#[tokio::test]
async fn errors_after_too_many_redirects() {
    let mut server = Server::new_async().await;
    let looping = server
        .mock("GET", "/loop")
        .with_status(302)
        .with_header("location", "/loop")
        .expect_at_least(2)
        .create_async()
        .await;

    let result = client().get(format!("{}/loop", server.url())).send().await;
    assert!(result.unwrap_err().is_redirect());
    looping.assert_async().await;
}
