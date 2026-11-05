use git_bot_feedback::client::{LocalClient, RestApiClient};

#[test]
fn local_is_none() {
    assert_eq!(LocalClient.api_url(), None);
}

#[test]
fn local_has_no_default_headers() {
    assert!(LocalClient.default_headers().unwrap().is_empty());
}

#[cfg(any(feature = "github", feature = "gitea", feature = "gitlab"))]
mod ci {
    use git_bot_feedback::client::init_client;
    use std::env;

    #[cfg(feature = "github")]
    #[test]
    fn github() {
        unsafe {
            env::set_var("GITHUB_ACTIONS", "true");
            env::remove_var("GITEA_ACTIONS");
            env::remove_var("FORGEJO_ACTIONS");
            env::remove_var("GITLAB_CI");
            env::set_var("GITHUB_REPOSITORY", "2bndy5/git-bot-feedback");
            env::set_var("GITHUB_SHA", "DEADBEEF");
            env::set_var("GITHUB_EVENT_NAME", "push");
            env::set_var("GITHUB_API_URL", "https://api.github.com");
            env::set_var("GITHUB_TOKEN", "secret");
        }
        let client = init_client().unwrap();
        assert_eq!(client.client_kind(), "github");
        assert_eq!(
            client.api_url().unwrap().as_str(),
            "https://api.github.com/"
        );
        let headers = client.default_headers().unwrap();
        assert_eq!(headers["accept"], "application/vnd.github.raw+json");
        assert_eq!(headers["authorization"], "token secret");
    }

    #[cfg(feature = "gitea")]
    #[test]
    fn gitea() {
        unsafe {
            env::set_var("GITEA_ACTIONS", "true");
            env::remove_var("FORGEJO_ACTIONS");
            env::remove_var("GITHUB_ACTIONS");
            env::remove_var("GITLAB_CI");
            env::set_var("GITEA_REPOSITORY", "2bndy5/git-bot-feedback");
            env::set_var("GITEA_SHA", "DEADBEEF");
            env::set_var("GITEA_EVENT_NAME", "push");
            env::set_var("GITEA_API_URL", "https://gitea.example.com");
            env::set_var("GITEA_TOKEN", "secret");
        }
        let client = init_client().unwrap();
        assert_eq!(client.client_kind(), "gitea");
        assert_eq!(
            client.api_url().unwrap().as_str(),
            "https://gitea.example.com/api/v1/"
        );
        let headers = client.default_headers().unwrap();
        assert_eq!(headers["accept"], "application/json");
        assert_eq!(headers["authorization"], "token secret");
    }

    #[cfg(feature = "gitlab")]
    #[test]
    fn gitlab() {
        unsafe {
            env::set_var("GITLAB_CI", "true");
            env::remove_var("GITHUB_ACTIONS");
            env::remove_var("GITEA_ACTIONS");
            env::remove_var("FORGEJO_ACTIONS");
            env::set_var("CI_API_V4_URL", "https://gitlab.example.com/api/v4");
            env::set_var("CI_PROJECT_ID", "42");
            env::set_var("CI_COMMIT_SHA", "DEADBEEF");
            env::set_var("GITLAB_TOKEN", "secret");
        }
        let client = init_client().unwrap();
        assert_eq!(client.client_kind(), "gitlab");
        assert_eq!(
            client.api_url().unwrap().as_str(),
            "https://gitlab.example.com/api/v4/"
        );
        let headers = client.default_headers().unwrap();
        assert_eq!(headers["accept"], "application/json");
        assert_eq!(headers["private-token"], "secret");
    }
}
