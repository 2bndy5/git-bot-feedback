use git_bot_feedback::client::{LocalClient, RestApiClient};

#[test]
fn local_is_none() {
    assert_eq!(LocalClient.workspace_root().unwrap(), None);
}

#[cfg(any(feature = "github", feature = "gitea"))]
mod ci {
    use git_bot_feedback::{RestClientError, client::init_client};
    use std::{env, path::PathBuf};

    const WORKSPACE: &str = "/home/runner/work/project";
    #[cfg(feature = "gitea")]
    const OTHER_WORKSPACE: &str = "/home/runner/work/other";

    fn assert_env_var_error(err: RestClientError, expected_name: &str) {
        match err {
            RestClientError::EnvVar { name, source } => {
                assert_eq!(name, expected_name);
                assert_eq!(source, env::VarError::NotPresent);
            }
            _ => panic!("expected an EnvVar error, got {err:?}"),
        }
    }

    #[cfg(feature = "github")]
    fn set_github_env(workspace: Option<&str>) {
        unsafe {
            env::set_var("GITHUB_ACTIONS", "true");
            env::remove_var("GITEA_ACTIONS");
            env::set_var("GITHUB_REPOSITORY", "2bndy5/git-bot-feedback");
            env::set_var("GITHUB_SHA", "DEADBEEF");
            env::set_var("GITHUB_EVENT_NAME", "push");
            env::set_var("GITHUB_API_URL", "https://api.github.com");
            match workspace {
                Some(path) => env::set_var("GITHUB_WORKSPACE", path),
                None => env::remove_var("GITHUB_WORKSPACE"),
            }
        }
    }

    #[cfg(feature = "github")]
    #[test]
    fn github_workspace() {
        set_github_env(Some(WORKSPACE));
        let client = init_client().unwrap();
        assert_eq!(client.client_kind(), "github");
        assert_eq!(
            client.workspace_root().unwrap(),
            Some(PathBuf::from(WORKSPACE))
        );
    }

    #[cfg(feature = "github")]
    #[test]
    fn github_without_workspace() {
        set_github_env(None);
        let client = init_client().unwrap();
        assert_env_var_error(client.workspace_root().unwrap_err(), "GITHUB_WORKSPACE");
    }

    #[cfg(feature = "gitea")]
    fn set_gitea_env(gitea_workspace: Option<&str>, github_workspace: Option<&str>) {
        unsafe {
            env::set_var("GITEA_ACTIONS", "true");
            env::set_var("GITEA_REPOSITORY", "2bndy5/git-bot-feedback");
            env::set_var("GITEA_SHA", "DEADBEEF");
            env::set_var("GITEA_EVENT_NAME", "push");
            env::set_var("GITEA_API_URL", "https://gitea.com/api/v1");
            for (name, value) in [
                ("GITEA_WORKSPACE", gitea_workspace),
                ("GITHUB_WORKSPACE", github_workspace),
            ] {
                match value {
                    Some(path) => env::set_var(name, path),
                    None => env::remove_var(name),
                }
            }
        }
    }

    #[cfg(feature = "gitea")]
    #[test]
    fn gitea_workspace() {
        set_gitea_env(Some(WORKSPACE), Some(OTHER_WORKSPACE));
        let client = init_client().unwrap();
        assert_eq!(client.client_kind(), "gitea");
        assert_eq!(
            client.workspace_root().unwrap(),
            Some(PathBuf::from(WORKSPACE))
        );
    }

    #[cfg(feature = "gitea")]
    #[test]
    fn gitea_falls_back_to_github_workspace() {
        set_gitea_env(None, Some(OTHER_WORKSPACE));
        let client = init_client().unwrap();
        assert_eq!(client.client_kind(), "gitea");
        assert_eq!(
            client.workspace_root().unwrap(),
            Some(PathBuf::from(OTHER_WORKSPACE))
        );
    }

    #[cfg(feature = "gitea")]
    #[test]
    fn gitea_without_workspace() {
        set_gitea_env(None, None);
        let client = init_client().unwrap();
        assert_eq!(client.client_kind(), "gitea");
        assert_env_var_error(
            client.workspace_root().unwrap_err(),
            "GITEA_WORKSPACE or GITHUB_WORKSPACE"
        );
    }
}
