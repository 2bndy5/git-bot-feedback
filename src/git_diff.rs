#[cfg(feature = "pyo3")]
use pyo3::prelude::*;

use regex::{Captures, Regex};
#[cfg(feature = "file-changes")]
use std::{collections::HashMap, ops::Range, path::Path};

use crate::error::DiffError;
#[cfg(feature = "file-changes")]
use crate::{FileDiffLines, FileFilter, LinesChangedOnly};

/// A struct to represent the header information of a diff hunk.
#[cfg_attr(
    feature = "pyo3",
    pyclass(module = "git_bot_feedback", from_py_object, get_all, frozen)
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffHunkHeader {
    /// The starting line number of the old hunk.
    pub old_start: u32,
    /// The total number of lines in the old hunk.
    pub old_lines: u32,
    /// The starting line number of the new hunk.
    pub new_start: u32,
    /// The total number of lines in the new hunk.
    pub new_lines: u32,
}

#[cfg(feature = "pyo3")]
#[pymethods]
impl DiffHunkHeader {
    /// Create a new diff hunk header instance.
    #[new]
    #[pyo3(text_signature = "(old_start: int, old_lines: int, new_start: int, new_lines: int)")]
    pub fn new_py(old_start: i64, old_lines: i64, new_start: i64, new_lines: i64) -> Self {
        Self {
            old_start: old_start.clamp(0, u32::MAX as i64) as u32,
            old_lines: old_lines.clamp(0, u32::MAX as i64) as u32,
            new_start: new_start.clamp(0, u32::MAX as i64) as u32,
            new_lines: new_lines.clamp(0, u32::MAX as i64) as u32,
        }
    }
}

impl DiffHunkHeader {
    /// Create a hunk header from the captures of [`HUNK_INFO_PATTERN`].
    ///
    /// A range without a count (eg. `-3` in `@@ -3 +3,2 @@`) implies a count of 1.
    fn from_captures(captures: Captures) -> Self {
        let [old_start, old_lines, new_start, new_lines] =
            captures.extract().1.map(|v| v.parse::<u32>().unwrap_or(1));
        Self {
            old_start,
            old_lines,
            new_start,
            new_lines,
        }
    }
}

fn get_filename_from_front_matter(front_matter: &str) -> Result<Option<&str>, DiffError> {
    let diff_file_name = Regex::new(r"(?m)^\+\+\+\sb?/(.*)$")?;
    if let Some(captures) = diff_file_name.captures(front_matter)
        && let Some(name) = captures.get(1)
    {
        return Ok(Some(name.as_str().trim_end_matches(['\r', '\n'])));
    }
    let diff_renamed_file = Regex::new(r"(?m)^rename to (.*)$")?;
    if front_matter.starts_with("similarity")
        && let Some(captures) = diff_renamed_file.captures(front_matter)
        && let Some(name) = captures.get(1)
    {
        return Ok(Some(name.as_str().trim_end_matches(['\r', '\n'])));
    }
    let diff_binary_file = Regex::new(r"(?m)^Binary\sfiles\s")?;
    let diff_mode_only = Regex::new(r"(?x)\Aold\ mode\ [0-7]{6}\r?\nnew\ mode\ [0-7]{6}\r?\n?\z")?;
    if diff_mode_only.is_match(front_matter) || diff_binary_file.is_match(front_matter) {
        return Ok(None);
    }
    Err(DiffError::MalformedDiffError(front_matter.to_string()))
}

/// A regex pattern used in multiple functions.
///
/// Captures the old start, old count, new start, and new count (in that order).
static HUNK_INFO_PATTERN: &str = r"(?m)^@@\s\-(\d+),?(\d*)\s\+(\d+),?(\d*)\s@@";

/// A single file's diff.
pub(crate) struct FileDiff<'a> {
    /// The file's name (in the new version).
    pub name: &'a str,
    /// Everything between the `diff --git` line and the first hunk.
    #[cfg(feature = "gitlab")]
    pub front_matter: &'a str,
    /// Each hunk's header and the text that follows it.
    ///
    /// The text starts with the rest of the header's line
    /// (eg. ` int main() {` in `@@ -3,7 +3,8 @@ int main() {`).
    pub hunks: Vec<(DiffHunkHeader, &'a str)>,
}

/// Split a git `diff` string into the diff of each file.
///
/// Files without a name (binary files and files with only a mode change) are excluded.
pub(crate) fn split_diff(diff: &str) -> Result<Vec<FileDiff<'_>>, DiffError> {
    let mut files = Vec::new();
    let diff_file_delimiter = Regex::new(r"(?m)^diff \-\-git a/.*$")?;
    let hunk_info = Regex::new(HUNK_INFO_PATTERN)?;

    for file_diff in diff_file_delimiter.split(diff) {
        if file_diff.is_empty() || file_diff.starts_with("deleted file") {
            continue;
        }
        let hunk_start = if let Some(first_hunk) = hunk_info.find(file_diff) {
            first_hunk.start()
        } else {
            file_diff.len()
        };
        let front_matter = &file_diff[..hunk_start];
        if let Some(file_name) = get_filename_from_front_matter(front_matter.trim_start())? {
            let patch = &file_diff[hunk_start..];
            // skip the first split because it is anything that precedes first hunk header
            let hunks = hunk_info
                .captures_iter(patch)
                .map(DiffHunkHeader::from_captures)
                .zip(hunk_info.split(patch).skip(1))
                .collect();
            files.push(FileDiff {
                name: file_name.strip_prefix('/').unwrap_or(file_name),
                #[cfg(feature = "gitlab")]
                front_matter,
                hunks,
            });
        }
    }
    Ok(files)
}

/// Parses a single file's hunks.
///
/// Returns a 2-item tuple:
///
/// - the line numbers that contain additions
/// - the ranges of lines that span each hunk
#[cfg(feature = "file-changes")]
fn parse_patch(hunks: &[(DiffHunkHeader, &str)]) -> (Vec<u32>, Vec<Range<u32>>) {
    let mut diff_hunks = Vec::new();
    let mut additions = Vec::new();
    for (header, hunk) in hunks {
        let mut line_numb_in_diff = header.new_start;
        diff_hunks.push(header.new_start..header.new_start + header.new_lines);
        for (line_index, line) in hunk.split('\n').enumerate() {
            if line.starts_with('+') {
                additions.push(line_numb_in_diff);
            }
            if line_index > 0 && !line.starts_with('-') {
                line_numb_in_diff += 1;
            }
        }
    }
    (additions, diff_hunks)
}

/// Parses a git `diff` string into a map of file names to their corresponding
/// [`FileDiffLines`].
///
/// The `file_filter` is used to filter out files that are not of interest.
/// The `lines_changed_only` parameter determines whether to include files
/// based on their contents' changes.
#[cfg(feature = "file-changes")]
pub fn parse_diff(
    diff: &str,
    file_filter: &FileFilter,
    lines_changed_only: &LinesChangedOnly,
) -> Result<HashMap<String, FileDiffLines>, DiffError> {
    let mut results = HashMap::new();
    for file in split_diff(diff)? {
        if file_filter.is_qualified(Path::new(file.name)) {
            let (added_lines, diff_hunks) = parse_patch(&file.hunks);
            if lines_changed_only.is_change_valid(!added_lines.is_empty(), !diff_hunks.is_empty()) {
                results
                    .entry(file.name.to_string())
                    .or_insert_with(|| FileDiffLines::with_info(added_lines, diff_hunks));
            }
        }
    }
    Ok(results)
}

// ******************* UNIT TESTS ***********************
#[cfg(all(test, feature = "file-changes"))]
mod test {
    #![allow(clippy::unwrap_used)]

    use super::parse_diff;
    use crate::{FileFilter, LinesChangedOnly, error::DiffError};

    const BAD_DIFF: &str = r#"{"message":"Resource not accessible by integration"}"#;

    #[test]
    fn bad_diff() {
        let files = parse_diff(
            BAD_DIFF,
            &FileFilter::new(&[], &["rs"], None),
            &LinesChangedOnly::Diff,
        );
        let e = files.unwrap_err();
        assert!(matches!(e, DiffError::MalformedDiffError(_)));
        assert!(e.to_string().ends_with(BAD_DIFF));
    }

    const RENAMED_DIFF: &str = r#"diff --git a/tests/demo/some source.cpp b/tests/demo/some source.c
similarity index 100%
rename from /tests/demo/some source.cpp
rename to /tests/demo/some source.c
diff --git a/some picture.png b/some picture.png
new file mode 100644
Binary files /dev/null and b/some picture.png differ
"#;

    #[test]
    fn parse_renamed_diff() {
        let files = parse_diff(
            RENAMED_DIFF,
            &FileFilter::new(&[], &["c"], None),
            &LinesChangedOnly::Off,
        )
        .unwrap();
        let git_file = files.get("tests/demo/some source.c").unwrap();
        assert!(git_file.added_lines.is_empty());
        assert!(git_file.diff_hunks.is_empty());
    }

    #[test]
    fn parse_renamed_only_diff() {
        let files = parse_diff(
            RENAMED_DIFF,
            &FileFilter::new(&[], &["c"], None),
            &LinesChangedOnly::Diff,
        )
        .unwrap();
        assert!(files.is_empty());
    }

    const RENAMED_DIFF_WITH_CHANGES: &str = r#"diff --git a/tests/demo/some source.cpp b/tests/demo/some source.c
similarity index 99%
rename from /tests/demo/some source.cpp
rename to /tests/demo/some source.c
@@ -3,7 +3,7 @@
\n \n \n-#include "math.h"
+#include <math.h>\n \n \n \n"#;

    #[test]
    fn parse_renamed_diff_with_patch() {
        let files = parse_diff(
            &String::from_iter([RENAMED_DIFF_WITH_CHANGES, TERSE_HEADERS]),
            // ignore src/demo.cpp file (in TERSE_HEADERS) via glob (src/*);
            // triggers code coverage of a `}` (region end)
            &FileFilter::new(&["src/*"], &["c", "cpp"], None),
            &LinesChangedOnly::On,
        )
        .unwrap();
        eprintln!("files: {files:#?}");
        let git_file = files.get("tests/demo/some source.c").unwrap();
        assert!(!git_file.is_line_in_diff(&1));
        assert!(git_file.is_line_in_diff(&4));
    }

    const TYPICAL_DIFF: &str = "diff --git a/path/for/Some file.cpp b/path/to/Some file.cpp\n\
                            --- a/path/for/Some file.cpp\n\
                            +++ b/path/to/Some file.cpp\n\
                            @@ -3,7 +3,7 @@\n \n \n \n\
                            -#include <some_lib/render/animation.hpp>\n\
                            +#include <some_lib/render/animations.hpp>\n \n \n \n";

    #[test]
    fn parse_typical_diff() {
        let files = parse_diff(
            TYPICAL_DIFF,
            &FileFilter::new(&[], &["cpp"], None),
            &LinesChangedOnly::On,
        )
        .unwrap();
        assert!(!files.is_empty());
    }

    const IGNORED_DIFF: &str = "diff --git a/some picture.png b/some picture.png\n\
                new file mode 100644\n\
                Binary files /dev/null and b/some picture.png differ\n\
                diff --git a/script.sh b/script.sh\n\
                old mode 100644\n\
                new mode 100755\n";

    #[test]
    fn parse_ignored_diff() {
        let files = parse_diff(
            IGNORED_DIFF,
            &FileFilter::new(&[], &["png"], None),
            &LinesChangedOnly::Diff,
        )
        .unwrap();
        assert!(files.is_empty());
    }

    const TERSE_HEADERS: &str = r#"diff --git a/src/demo.cpp b/src/demo.cpp
--- a/src/demo.cpp
+++ b/src/demo.cpp
@@ -3 +3 @@
-#include <stdio.h>
+#include "stdio.h"
@@ -4,0 +5,2 @@
+auto main() -> int
+{
@@ -18 +17,2 @@ int main(){
-    return 0;}
+    return 0;
+}"#;

    #[test]
    fn terse_hunk_header() {
        let file_filter = FileFilter::new(&[], &["cpp"], None);
        let files = parse_diff(TERSE_HEADERS, &file_filter, &LinesChangedOnly::Diff).unwrap();
        let file_diff = files.get("src/demo.cpp").unwrap();
        assert_eq!(file_diff.diff_hunks, vec![3..4, 5..7, 17..19]);
    }

    #[test]
    fn hunk_header_in_content() {
        // A line that looks like a hunk header is not one unless it starts the line.
        let diff = "diff --git a/a.cpp b/a.cpp\n--- a/a.cpp\n+++ b/a.cpp\n@@ -1 +1,2 @@\n+// @@ -9 +9 @@\n context\n";
        let files = parse_diff(
            diff,
            &FileFilter::new(&[], &["cpp"], None),
            &LinesChangedOnly::Diff,
        )
        .unwrap();
        let file_diff = files.get("a.cpp").unwrap();
        assert_eq!(file_diff.diff_hunks, vec![1..3]);
        assert_eq!(file_diff.added_lines, vec![1]);
    }
}
