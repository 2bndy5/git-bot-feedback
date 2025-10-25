use fast_glob::glob_match;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};
use tokio::fs;

use super::FileDiffLines;
use crate::error::DirWalkError;

/// A structure to encapsulate file path filtering behavior.
#[derive(Debug, Clone, Default)]
#[cfg_attr(docsrs, doc(cfg(feature = "file-changes")))]
pub struct FileFilter {
    /// A set of paths or glob patterns to be ignored.
    ///
    /// These paths/patterns are relative to the working directory.
    /// An empty entry represents the working directory itself.
    pub ignored: HashSet<String>,

    /// A set of paths or glob patterns to be explicitly not ignored.
    ///
    /// These paths/patterns are relative to the working directory.
    /// An empty entry represents the working directory itself.
    pub not_ignored: HashSet<String>,

    /// A set of valid file extensions.
    ///
    /// These extensions do not include the leading dot.
    /// For example, use "txt" instead of ".txt".
    ///
    /// A blank extension (`""`) can be used to match files with
    /// no extension (eg. ".clang-format").
    pub extensions: HashSet<String>,

    /// An optional scope name for logging purposes.
    log_scope: Option<String>,
}

impl FileFilter {
    /// Convenience constructor to instantiate a [`FileFilter`] object.
    ///
    /// The `ignore` parameter is a list of paths (or glob patterns).
    /// A path or pattern is explicitly not ignored if it is prefixed with `!`.
    /// Otherwise it is ignored.
    ///
    /// Leading and trailing spaces are stripped from each item in the `ignore` list.
    /// Also, leading `./` sequences are stripped.
    ///
    /// ```
    /// #[cfg(feature = "file-changes")]
    /// use git_bot_feedback::FileFilter;
    /// let filter = FileFilter::new(
    ///     &[" src ", " ! src/lib.rs "],
    ///     &["rs", "toml"],
    ///     None,
    /// );
    /// assert!(filter.ignored.contains("src"));
    /// assert!(filter.not_ignored.contains("src/lib.rs"));
    /// ```
    pub fn new(ignore: &[&str], extensions: &[&str], log_scope: Option<&str>) -> Self {
        let (ignored, not_ignored) = Self::parse_ignore(ignore);
        let extensions = HashSet::from_iter(extensions.iter().map(|v| v.to_string()));
        Self {
            ignored,
            not_ignored,
            extensions,
            log_scope: log_scope.map(|s| s.to_string()),
        }
    }

    /// This will parse the list of paths specified using [`Self::new`]'s `ignore`
    /// argument.
    ///
    /// It returns 2 sets (in order):
    ///
    /// - [`Self::ignored`] paths/patterns
    /// - [`Self::not_ignored`] paths/patterns
    fn parse_ignore(ignore: &[&str]) -> (HashSet<String>, HashSet<String>) {
        let mut ignored = HashSet::new();
        let mut not_ignored = HashSet::new();
        for pattern in ignore {
            let as_posix = pattern.replace('\\', "/");
            let mut pat = as_posix.as_str().trim();
            let is_ignored = !pat.starts_with('!');
            if !is_ignored {
                pat = pat[1..].trim_start();
            }
            if pat.starts_with("./") {
                pat = &pat[2..];
            }
            if is_ignored {
                ignored.insert(pat.to_string());
            } else {
                not_ignored.insert(pat.to_string());
            }
        }
        (ignored, not_ignored)
    }

    /// This function will read a .gitmodules file located in the working directory.
    /// The named submodules' paths will be automatically added to the [`FileFilter::ignored`] set,
    /// unless the submodule's path is already specified in the [`FileFilter::not_ignored`] set.
    pub async fn parse_submodules(&mut self) {
        if let Ok(read_buf) = fs::read_to_string(".gitmodules").await {
            for line in read_buf.split('\n') {
                let line_trimmed = line.trim();
                if line_trimmed.starts_with("path") {
                    // .gitmodules convention defines path to submodule as `path = submodule_path`
                    if let Some(path) = line_trimmed
                        .splitn(2, '=') // can be less than 2 items
                        .skip(1) // skip first to ensure that
                        .last() // last() returns the second item (or None)
                        .map(|v| v.trim_start())
                        && !path.is_empty()
                    {
                        let submodule = path.to_string();
                        log::debug!("Found submodule in path: {submodule}");
                        let mut is_ignored = true;
                        for pat in &self.not_ignored {
                            if pat == &submodule {
                                is_ignored = false;
                                break;
                            }
                        }
                        if is_ignored && !self.ignored.contains(&submodule) {
                            self.ignored.insert(submodule);
                        }
                    } else {
                        log::error!("Failed to parse submodule path from line: {line}");
                    }
                }
            }
        }
    }

    /// Describes if a specified `file_name` is contained within the specified set of paths.
    ///
    /// The `is_ignored` flag describes which set of paths is used as domains.
    /// The specified `file_name` can be a direct or distant descendant of any
    /// paths in the set.
    ///
    /// Returns a `true` value of the the path/pattern that matches the given `file_name`.
    /// If given `file_name` is not in the specified set, then `false` is returned.
    pub fn is_file_in_list(&self, file_name: &Path, is_ignored: bool) -> bool {
        let file_name = PathBuf::from(
            file_name
                .as_os_str()
                .to_string_lossy()
                .to_string()
                .replace("\\", "/")
                .trim_start_matches("./"),
        );
        let set = if is_ignored {
            &self.ignored
        } else {
            &self.not_ignored
        };
        for pattern in set {
            let pat = PathBuf::from(&pattern);
            if pattern.is_empty()
                || glob_match(pattern, file_name.to_string_lossy().as_ref())
                || (pat.is_file() && file_name == pat)
                || (pat.is_dir() && file_name.starts_with(pat))
            {
                log::debug!(
                    "{}file {file_name:?} is {}ignored with domain {pattern:?}.",
                    if let Some(scope) = &self.log_scope {
                        format!("({}) ", scope)
                    } else {
                        "".to_string()
                    },
                    if is_ignored { "" } else { "not " }
                );
                return true;
            }
        }
        false
    }

    /// Convenience function to check if a given `file_name` is ignored.
    ///
    /// Equivalent to calling
    /// [`file_filter.is_file_in_list(file_name, true)`](Self::is_file_in_list).
    pub fn is_file_ignored(&self, file_name: &Path) -> bool {
        self.is_file_in_list(file_name, true)
    }

    /// Convenience function to check if a given `file_name` is *not* ignored.
    ///
    /// Equivalent to calling
    /// [`file_filter.is_file_in_list(file_name, false)`](Self::is_file_in_list).
    pub fn is_file_not_ignored(&self, file_name: &Path) -> bool {
        self.is_file_in_list(file_name, false)
    }

    /// A function that checks if `file_path` satisfies the following conditions (in
    /// ordered priority):
    ///
    /// - Does `file_path` use at least 1 of [`FileFilter::extensions`]?
    ///   Not applicable if [`FileFilter::extensions`] is empty.
    /// - Is `file_path` specified in [`FileFilter::not_ignored`]?
    /// - Is `file_path` *not* specified in [`FileFilter::ignored`]?
    /// - Is `file_path` not a hidden path (any parts of the path start with ".")?
    ///   Mutually exclusive with last condition; does not apply to "./" or "../".
    pub fn is_not_ignored(&self, file_path: &Path) -> bool {
        if !self.extensions.is_empty() && !file_path.is_dir() {
            let extension = file_path
                .extension()
                .unwrap_or_default() // allow for matching files with no extension
                .to_string_lossy()
                .to_string();
            if !self.extensions.contains(&extension) {
                return false;
            }
        }
        let is_not_ignored = self.is_file_not_ignored(file_path);
        is_not_ignored || {
            // if not explicitly unignored
            let is_ignored = self.is_file_ignored(file_path);
            let is_hidden = file_path.components().any(|c| {
                let comp = c.as_os_str().to_string_lossy();
                comp.starts_with('.') && !["..", "."].contains(&comp.as_ref())
            });
            // is implicitly not ignored and not a hidden file/folder
            !is_ignored && !is_hidden
        }
    }

    /// Walks a given `root_path` recursively and returns a map of discovered source files.
    ///
    /// Each entry in the returned map is comprises the discovered file's path (as key) and
    /// an empty [`FileDiffLines`] object (as value). Only files that satisfy the following
    /// conditions are included in the returned map:
    ///
    /// - uses at least 1 of the given [`FileFilter::extensions`].
    /// - is specified in the internal list [`FileFilter::not_ignored`] paths/patterns
    /// - is not specified in the set of [`FileFilter::ignored`] paths/patterns and
    ///   is not a hidden path (starts with ".").
    pub async fn walk_dir(
        &self,
        root_path: &str,
    ) -> Result<HashMap<String, FileDiffLines>, DirWalkError> {
        let mut files: HashMap<String, FileDiffLines> = HashMap::new();
        let mut entries = fs::read_dir(root_path)
            .await
            .map_err(|e| DirWalkError::ReadDirError(root_path.to_string(), e))?;
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.is_dir() {
                files.extend(Box::pin(self.walk_dir(&path.to_string_lossy())).await?);
            } else {
                let is_valid_src = self.is_not_ignored(&path);
                if is_valid_src {
                    let file_name = path
                        .clone()
                        .to_string_lossy()
                        .replace("\\", "/")
                        .trim_start_matches("./")
                        .to_string();
                    files.entry(file_name).or_default();
                }
            }
        }
        Ok(files)
    }
}

#[cfg(test)]
mod tests {
    use super::FileFilter;
    use std::{
        env::set_current_dir,
        path::{Path, PathBuf},
    };

    // ************* tests for ignored paths

    fn setup_ignore(input: &str, extension: &[&str]) -> FileFilter {
        let ignore: Vec<&str> = input.split('|').collect();
        let file_filter = FileFilter::new(&ignore, extension, None);
        println!("ignored = {:?}", file_filter.ignored);
        println!("not ignored = {:?}", file_filter.not_ignored);
        file_filter
    }

    #[test]
    fn ignore_src() {
        let file_filter = setup_ignore("src", &[]);
        assert!(file_filter.is_file_ignored(&PathBuf::from("./src/lib.rs")));
        assert!(!file_filter.is_file_not_ignored(&PathBuf::from("./src/lib.rs")));
    }

    #[test]
    fn ignore_root() {
        let file_filter = setup_ignore("! src/lib.rs | ./", &[]);
        assert!(file_filter.is_file_ignored(&PathBuf::from("./Cargo.toml")));
        assert!(file_filter.is_file_not_ignored(&PathBuf::from("./src/lib.rs")));
    }

    #[test]
    fn ignore_root_implicit() {
        let file_filter = setup_ignore("!src|", &[]);
        assert!(file_filter.is_file_ignored(&PathBuf::from("./Cargo.toml")));
        assert!(file_filter.is_file_not_ignored(&PathBuf::from("./src/lib.rs")));
    }

    #[test]
    fn ignore_glob() {
        let file_filter = setup_ignore("!src/**/*", &[]);
        assert!(file_filter.is_file_not_ignored(&PathBuf::from("./src/lib.rs")));
        assert!(file_filter.is_file_not_ignored(&PathBuf::from("./src/file_utils/file_filter.rs")));
    }

    #[tokio::test]
    async fn ignore_submodules() {
        let mut file_filter = setup_ignore("!pybind11", &[]);
        file_filter.parse_submodules().await;
        assert!(file_filter.ignored.is_empty());
        assert!(file_filter.is_file_not_ignored(&Path::new("pybind11")));
        set_current_dir("tests/assets/ignored_paths/error").unwrap();
        file_filter.parse_submodules().await;
        assert!(file_filter.ignored.is_empty());
        set_current_dir("../").unwrap();
        file_filter.parse_submodules().await;
        println!("submodules ignored = {:?}", file_filter.ignored);

        // using Vec::contains() because these files don't actually exist in project files
        for ignored_submodule in ["RF24", "RF24Network", "RF24Mesh"] {
            assert!(file_filter.ignored.contains(ignored_submodule));
            assert!(
                !file_filter
                    .is_file_ignored(&PathBuf::from(ignored_submodule).join("some_src.cpp"))
            );
        }
        assert!(file_filter.not_ignored.contains(&"pybind11".to_string()));
        assert!(!file_filter.is_file_not_ignored(&PathBuf::from("pybind11/some_src.cpp")));
    }

    // *********************** tests for recursive path search

    #[tokio::test]
    async fn walk_dir_recursively() {
        let extensions = vec!["txt", "json"];
        let file_filter = setup_ignore("target", &extensions);
        let files = file_filter.walk_dir(".").await.unwrap();
        println!("discovered files: {:?}", files.keys());
        assert!(!files.is_empty());
        for (file, diff_lines) in files {
            let ext = PathBuf::from(&file)
                .extension()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            assert!(extensions.contains(&ext.as_str()));
            assert!(!file.contains("\\"));
            assert!(!file.starts_with("./"));
            assert!(diff_lines.added_lines.is_empty());
            assert!(diff_lines.diff_hunks.is_empty());
        }
        assert!(!file_filter.is_file_not_ignored(&Path::new(
            "tests/assets/ignored_paths/.hidden/ignore_me.txt"
        )));
        assert!(!file_filter.is_not_ignored(&Path::new("tests/assets/ignored_paths/.hidden")));
        assert!(file_filter.is_not_ignored(&Path::new("tests/assets/ignored_paths")));
    }
}
