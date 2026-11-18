use super::RestApiClient;
use crate::{
    FileAnnotation, OutputVariable, RestClientError as ClientError, ReviewOptions,
    ThreadCommentOptions,
};
use std::io::{Write, stdout};

#[cfg(feature = "file-changes")]
use crate::{FileDiffLines, FileFilter, LinesChangedOnly, parse_diff};
#[cfg(feature = "file-changes")]
use std::{collections::HashMap, path::Path, process::Command};

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

#[cfg(all(test, feature = "file-changes"))]
mod test {
    use super::count_staged_entries;

    #[test]
    fn staged_entries() {
        let status = " M src/a.cpp\nM  src/b.cpp\nMM src/c.cpp\nA  src/d.cpp\n?? src/e.cpp\n?? results.sarif\n";
        // b, c and d have staged changes; a is only modified in the worktree, e and results.sarif are untracked.
        assert_eq!(count_staged_entries(status), 3);
        assert_eq!(count_staged_entries("?? new.cpp\n"), 0);
        assert_eq!(count_staged_entries(""), 0);
    }
}

/// Helper function to resolve a git reference to a commit hash using `git rev-parse`.
#[cfg(feature = "file-changes")]
fn git_rev_parse(base: &str) -> Result<String, ClientError> {
    match Command::new("git").args(["rev-parse", base]).output() {
        Err(e) => Err(ClientError::Io {
            task: format!("invoke `git rev-parse {base}` to validate reference"),
            source: e,
        }),
        Ok(output) => {
            if output.status.success() {
                Ok(String::from_utf8_lossy(output.stdout.trim_ascii()).to_string())
            } else if base.chars().all(|c| c.is_ascii_digit()) {
                // if all chars from a decimal number, then
                // try using it as a number of parents from HEAD.
                // This is not infinitely recursive because
                // the adapted `base` is not all digits.
                git_rev_parse(format!("HEAD~{base}").as_str())
            } else {
                let err_msg = String::from_utf8_lossy(&output.stderr).to_string();
                // Given diff base did not resolve to a valid git reference
                Err(ClientError::GitCommand(err_msg))
            }
        }
    }
}

/// Count the entries of `git status --short` output that have staged changes.
///
/// The first status column is ` ` for an entry with no staged changes
/// and `?` for an untracked file; neither is staged.
#[cfg(feature = "file-changes")]
fn count_staged_entries(git_status: &str) -> usize {
    git_status
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with(' ') && !l.starts_with('?'))
        .count()
}

/// Get the diff of each untracked (and not ignored) file, as if each was a new file.
///
/// Files are first checked against the `file_filter`,
/// so only qualified files are read.
#[cfg(feature = "file-changes")]
fn untracked_files(
    file_filter: &FileFilter,
    lines_changed_only: &LinesChangedOnly,
) -> Result<HashMap<String, FileDiffLines>, ClientError> {
    // Paths in `git diff` output are relative to the repository root,
    // so list and diff the untracked files from the repository root too.
    let repo_root = match Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
    {
        Err(e) => return Err(ClientError::io("invoke `git rev-parse --show-toplevel`", e)),
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(output.stdout.trim_ascii()).to_string()
        }
        Ok(output) => {
            let err_msg = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(ClientError::GitCommand(err_msg));
        }
    };
    let names = match Command::new("git")
        .current_dir(&repo_root)
        .args(["ls-files", "--others", "--exclude-standard", "-z"])
        .output()
    {
        Err(e) => return Err(ClientError::io("invoke `git ls-files --others`", e)),
        Ok(output) if output.status.success() => output.stdout,
        Ok(output) => {
            let err_msg = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(ClientError::GitCommand(err_msg));
        }
    };
    let mut files = HashMap::new();
    for name in names.split(|b| *b == 0).filter(|n| !n.is_empty()) {
        let name = String::from_utf8_lossy(name).to_string();
        if !file_filter.is_qualified(Path::new(&name)) {
            continue;
        }
        log::debug!(
            "Getting diff of untracked file with `git diff --no-index -- /dev/null {name}`"
        );
        // `--no-index` exits with 1 when the files differ, so the exit code is not checked.
        // A failure to diff shows up as empty output, which adds no file.
        let output = Command::new("git")
            .current_dir(&repo_root)
            .args([
                "--no-pager",
                "diff",
                "--no-index",
                "--no-color",
                "--",
                "/dev/null",
            ])
            .arg(&name)
            .output()
            .map_err(|e| ClientError::io("invoke `git diff --no-index`", e))?;
        let diff_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
        files.extend(parse_diff(&diff_str, file_filter, lines_changed_only)?);
    }
    Ok(files)
}

#[async_trait::async_trait]
impl RestApiClient for LocalClient {
    fn client_kind(&self) -> String {
        "local".to_string()
    }

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
                        count_staged_entries(&String::from_utf8_lossy(&output.stdout))
                    } else {
                        let err_msg = String::from_utf8_lossy(&output.stderr).to_string();
                        return Err(ClientError::GitCommand(err_msg));
                    }
                }
            }
        };
        let mut diff_args = vec![];
        let mut git_sub_cmd = vec!["--no-pager"];
        if git_status != 0 {
            // There are changes in the working directory.
            // So, compare include the staged changes.
            diff_args.push("--staged".to_string());
        }
        if let Some(base) = base_diff {
            let resolved_base = git_rev_parse(&base)?;
            diff_args.push(resolved_base);
        } else if git_status == 0 {
            // No base diff provided and there are no staged changes,
            // just get the diff of the last commit.
            let resolved_head = git_rev_parse("HEAD~1")?;
            diff_args.push(resolved_head);
        }
        if ignore_index {
            // When ignoring the index, we want to compare
            // the working directory changes, not the staged changes.
            diff_args.push("--format=".to_string());
            git_sub_cmd.push("show");
        } else {
            git_sub_cmd.push("diff");
        };
        log::debug!(
            "Getting diff with `git {} {}`",
            git_sub_cmd.join(" "),
            diff_args.join(" ")
        );
        match Command::new("git")
            .args(&git_sub_cmd)
            .args(&diff_args)
            .output()
        {
            Err(e) => Err(ClientError::Io {
                task: format!(
                    "invoke `git {} {}`",
                    git_sub_cmd.join(" "),
                    diff_args.join(" ")
                ),
                source: e,
            }),
            Ok(output) => {
                if output.status.success() {
                    let diff_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    let mut files = parse_diff(&diff_str, file_filter, lines_changed_only)?;
                    if git_status == 0 && !ignore_index {
                        // Nothing is staged, so the diff is of the working tree.
                        // `git diff` never lists untracked files; add them as new files.
                        for (name, diff_lines) in untracked_files(file_filter, lines_changed_only)?
                        {
                            files.entry(name).or_insert(diff_lines);
                        }
                    }
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

    /// Log groups only mean something in a CI log, so this logs the group's name at debug level.
    fn start_log_group(&self, name: &str) {
        log::debug!("Start of {name}");
    }

    /// Log groups only mean something in a CI log, so this logs the group's name at debug level.
    fn end_log_group(&self, name: &str) {
        log::debug!("End of {name}");
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

    /// Print each annotation to stdout as one line (see [`FileAnnotation::fmt_local()`]).
    fn write_file_annotations(&self, annotations: &[FileAnnotation]) -> Result<(), ClientError> {
        let mut out = stdout().lock();
        for annotation in annotations {
            writeln!(out, "{}", annotation.fmt_local())
                .map_err(|e| ClientError::io("write file annotations to stdout", e))?;
        }
        out.flush()
            .map_err(|e| ClientError::io("flush stdout with file annotations", e))
    }

    fn write_output_variables(&self, vars: &[OutputVariable]) -> Result<(), ClientError> {
        for var in vars {
            log::info!("{}: {}", var.name, var.value);
        }
        Ok(())
    }
}
