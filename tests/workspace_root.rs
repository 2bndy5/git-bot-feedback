use git_bot_feedback::client::{LocalClient, RestApiClient};
use std::{
    env, fs,
    path::{Path, PathBuf},
};
use tempfile::{TempDir, tempdir};

/// A git repository (with a `.git` folder) that has a `member/src` sub-folder.
fn make_repo() -> (TempDir, PathBuf) {
    let tmp = tempdir().unwrap();
    let repo = tmp.path().join("repo");
    fs::create_dir_all(repo.join(".git")).unwrap();
    fs::create_dir_all(repo.join("member/src")).unwrap();
    (tmp, repo)
}

/// Compare paths after resolving symbolic links (eg. `/var` -> `/private/var` on macOS).
fn assert_same_path(actual: &Path, expected: &Path) {
    assert_eq!(
        fs::canonicalize(actual).unwrap(),
        fs::canonicalize(expected).unwrap()
    );
}

#[test]
fn local_from_sub_folder() {
    let (_tmp, repo) = make_repo();
    env::set_current_dir(repo.join("member/src")).unwrap();
    assert_same_path(&LocalClient.workspace_root(), &repo);
}

#[test]
fn local_without_git() {
    let tmp = tempdir().unwrap();
    let folder = tmp.path().join("not-a-repo");
    fs::create_dir_all(&folder).unwrap();
    env::set_current_dir(&folder).unwrap();
    let root = LocalClient.workspace_root();
    // the temp dir is not expected to be inside a git repository
    if fs::canonicalize(&root)
        .unwrap()
        .starts_with(fs::canonicalize(tmp.path()).unwrap())
    {
        assert_same_path(&root, &folder);
    }
}

#[cfg(any(feature = "github", feature = "gitea"))]
mod ci {
    use super::{assert_same_path, make_repo};
    use git_bot_feedback::client::init_client;
    use std::{env, path::Path};

    fn set_github_env(workspace: Option<&Path>) {
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
        let (_tmp, repo) = make_repo();
        // checked out into a sub-folder of the workspace (`actions/checkout` with `path`)
        env::set_current_dir(repo.join("member")).unwrap();
        set_github_env(Some(repo.parent().unwrap()));
        let client = init_client().unwrap();
        assert_eq!(client.client_kind(), "github");
        assert_eq!(client.workspace_root(), repo.parent().unwrap());
    }

    #[cfg(feature = "github")]
    #[test]
    fn github_without_workspace() {
        let (_tmp, repo) = make_repo();
        env::set_current_dir(repo.join("member")).unwrap();
        set_github_env(None);
        let client = init_client().unwrap();
        assert_same_path(&client.workspace_root(), &repo);
    }

    #[cfg(feature = "gitea")]
    #[test]
    fn gitea_workspace() {
        let (_tmp, repo) = make_repo();
        env::set_current_dir(repo.join("member")).unwrap();
        unsafe {
            env::set_var("GITEA_ACTIONS", "true");
            env::set_var("GITEA_REPOSITORY", "2bndy5/git-bot-feedback");
            env::set_var("GITEA_SHA", "DEADBEEF");
            env::set_var("GITEA_EVENT_NAME", "push");
            env::set_var("GITEA_API_URL", "https://gitea.com/api/v1");
            env::remove_var("GITEA_WORKSPACE");
            env::set_var("GITHUB_WORKSPACE", repo.join("member"));
        }
        let client = init_client().unwrap();
        assert_eq!(client.client_kind(), "gitea");
        // falls back to `GITHUB_WORKSPACE`
        assert_eq!(client.workspace_root(), repo.join("member"));
        unsafe {
            env::set_var("GITEA_WORKSPACE", &repo);
        }
        assert_eq!(client.workspace_root(), repo);
    }
}
