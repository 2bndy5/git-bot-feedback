use super::RestApiClient;
use crate::{OutputVariable, RestClientError as ClientError, ReviewOptions, ThreadCommentOptions};

#[cfg(feature = "file-changes")]
use crate::{FileDiffLines, FileFilter, LinesChangedOnly, parse_diff};
#[cfg(feature = "file-changes")]
use std::{collections::HashMap, process::Command};

/// A (mostly) non-operational implementation of [`RestApiClient`].
///
/// This is primarily meant for use in local contexts (or in unsupported CI
/// platforms/contexts) because the following methods silently do nothing:
///
/// - [`Self::post_thread_comment`]
/// - [`Self::cull_pr_reviews`]
/// - [`Self::post_pr_review`]
/// - [`Self::set_user_agent`]
///
/// However, [`Self::get_list_of_changed_files`] does use the git CLI
/// to get a list of changed files.
///
/// Instantiate with [`Default::default()`].
/// ```rust
/// use git_bot_feedback::client::LocalClient;
///
/// let client = LocalClient::default();
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LocalClient;

#[async_trait::async_trait]
impl RestApiClient for LocalClient {
    #[cfg(feature = "file-changes")]
    async fn get_list_of_changed_files(
        &self,
        file_filter: &FileFilter,
        lines_changed_only: &LinesChangedOnly,
        base_diff: Option<String>,
        ignore_index: bool,
    ) -> Result<HashMap<String, FileDiffLines>, ClientError> {
        let git_status = if ignore_index {
            0
        } else {
            match Command::new("git").args(["status", "--short"]).output() {
                Err(e) => {
                    return Err(ClientError::io("invoke `git status`", e));
                }
                Ok(output) => {
                    if output.status.success() {
                        String::from_utf8_lossy(&output.stdout)
                            .to_string()
                            // trim last newline to prevent an extra empty line being counted as a changed file
                            .trim_end_matches('\n')
                            .lines()
                            // we only care about staged changes
                            .filter(|l| !l.starts_with(' '))
                            .count()
                    } else {
                        let err_msg = String::from_utf8_lossy(&output.stderr).to_string();
                        return Err(ClientError::GitCommand(err_msg));
                    }
                }
            }
        };
        let mut diff_args = vec!["diff".to_string()];
        if git_status != 0 {
            // There are changes in the working directory.
            // So, compare include the staged changes.
            diff_args.push("--staged".to_string());
        }
        if let Some(base) = base_diff {
            match Command::new("git")
                .args(["rev-parse", base.as_str()])
                .output()
            {
                Err(e) => {
                    return Err(ClientError::Io {
                        task: format!("invoke `git rev-parse {base}` to validate reference"),
                        source: e,
                    });
                }
                Ok(output) => {
                    if output.status.success() {
                        diff_args.push(base);
                    } else if base.chars().all(|c| c.is_ascii_digit()) {
                        // if all chars form a decimal number, then
                        // try using it as a number of parents from HEAD
                        diff_args.push(format!("HEAD~{base}"));
                        // note, if still not a valid git reference, then
                        // the error will be raised by the `git diff` command later
                    } else {
                        let err_msg = String::from_utf8_lossy(&output.stderr).to_string();
                        // Given diff base did not resolve to a valid git reference
                        return Err(ClientError::GitCommand(err_msg));
                    }
                }
            }
        } else if git_status == 0 {
            // No base diff provided and there are no staged changes,
            // just get the diff of the last commit.
            diff_args.push("HEAD~1".to_string());
        }
        match Command::new("git").args(&diff_args).output() {
            Err(e) => Err(ClientError::Io {
                task: format!("invoke `git {}`", diff_args.join(" ")),
                source: e,
            }),
            Ok(output) => {
                if output.status.success() {
                    let diff_str = String::from_utf8_lossy(&output.stdout).to_string();
                    let files = parse_diff(&diff_str, file_filter, lines_changed_only)?;
                    Ok(files)
                } else {
                    let err_msg = String::from_utf8_lossy(&output.stderr).to_string();
                    Err(ClientError::GitCommand(err_msg))
                }
            }
        }
    }

    fn is_pr_event(&self) -> bool {
        false
    }

    fn set_user_agent(&mut self, _user_agent: &str) -> Result<(), ClientError> {
        Ok(())
    }

    async fn post_thread_comment(&self, _options: ThreadCommentOptions) -> Result<(), ClientError> {
        Ok(())
    }

    async fn cull_pr_reviews(&mut self, _options: &mut ReviewOptions) -> Result<(), ClientError> {
        Ok(())
    }

    async fn post_pr_review(&mut self, _options: &ReviewOptions) -> Result<(), ClientError> {
        Ok(())
    }

    fn write_output_variables(&self, vars: &[OutputVariable]) -> Result<(), ClientError> {
        for var in vars {
            log::info!("{}: {}", var.name, var.value);
        }
        Ok(())
    }
}
