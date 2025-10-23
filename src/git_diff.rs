use regex::Regex;
use std::{collections::HashMap, ops::Range, path::PathBuf};

use crate::{FileDiffLines, FileFilter, LinesChangedOnly};

/// A struct to represent the header information of a diff hunk.
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

fn get_filename_from_front_matter(front_matter: &str) -> Option<&str> {
    let diff_file_name = Regex::new(r"(?m)^\+\+\+\sb?/(.*)$").unwrap();
    let diff_renamed_file = Regex::new(r"(?m)^rename to (.*)$").unwrap();
    let diff_binary_file = Regex::new(r"(?m)^Binary\sfiles\s").unwrap();
    if let Some(captures) = diff_file_name.captures(front_matter) {
        return Some(captures.get(1).unwrap().as_str());
    }
    if front_matter.starts_with("similarity")
        && let Some(captures) = diff_renamed_file.captures(front_matter)
    {
        return Some(captures.get(1).unwrap().as_str());
    }
    if !diff_binary_file.is_match(front_matter) {
        log::warn!("Unrecognized diff starting with:\n{}", front_matter);
    }
    None
}

/// A regex pattern used in multiple functions
static HUNK_INFO_PATTERN: &str = r"(?m)@@\s\-\d+,?\d*\s\+(\d+),?(\d*)\s@@";

/// Parses a single file's patch containing one or more hunks
///
/// Returns a 2-item tuple:
///
/// - the line numbers that contain additions
/// - the ranges of lines that span each hunk
fn parse_patch(patch: &str) -> (Vec<u32>, Vec<Range<u32>>) {
    let mut diff_hunks = Vec::new();
    let mut additions = Vec::new();

    let hunk_info = Regex::new(HUNK_INFO_PATTERN).unwrap();
    let hunk_headers = hunk_info.captures_iter(patch).collect::<Vec<_>>();
    if !hunk_headers.is_empty() {
        // skip the first split because it is anything that precedes first hunk header
        let hunks = hunk_info.split(patch).skip(1);
        for (hunk, header) in hunks.zip(hunk_headers) {
            // header.unwrap() is safe because the hunk_headers.iter() is parallel to hunk_info.split()
            let [start_line, end_range] = header.extract().1.map(|v| v.parse::<u32>().unwrap_or(1));
            let mut line_numb_in_diff = start_line;
            diff_hunks.push(start_line..start_line + end_range);
            for (line_index, line) in hunk.split('\n').enumerate() {
                if line.starts_with('+') {
                    additions.push(line_numb_in_diff);
                }
                if line_index > 0 && !line.starts_with('-') {
                    line_numb_in_diff += 1;
                }
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
pub fn parse_diff(
    diff: &str,
    file_filter: &FileFilter,
    lines_changed_only: &LinesChangedOnly,
) -> HashMap<String, FileDiffLines> {
    let mut results = HashMap::new();
    let diff_file_delimiter = Regex::new(r"(?m)^diff --git a/.*$").unwrap();
    let hunk_info = Regex::new(HUNK_INFO_PATTERN).unwrap();

    let file_diffs = diff_file_delimiter.split(diff);
    for file_diff in file_diffs {
        if file_diff.is_empty() || file_diff.starts_with("deleted file") {
            continue;
        }
        let hunk_start = if let Some(first_hunk) = hunk_info.find(file_diff) {
            first_hunk.start()
        } else {
            file_diff.len()
        };
        let front_matter = &file_diff[..hunk_start];
        if let Some(file_name) = get_filename_from_front_matter(front_matter.trim_start()) {
            let file_name = file_name.strip_prefix('/').unwrap_or(file_name);
            let file_path = PathBuf::from(file_name);
            if file_filter.is_ext_and_not_ignored(&file_path) {
                let (added_lines, diff_hunks) = parse_patch(&file_diff[hunk_start..]);
                if lines_changed_only
                    .is_change_valid(!added_lines.is_empty(), !diff_hunks.is_empty())
                {
                    results
                        .entry(file_name.to_string())
                        .or_insert_with(|| FileDiffLines::with_info(added_lines, diff_hunks));
                }
            }
        }
    }
    results
}

// ******************* UNIT TESTS ***********************
#[cfg(test)]
mod test {
    use super::parse_diff;
    use crate::{FileFilter, LinesChangedOnly};

    const RENAMED_DIFF: &'static str = r#"diff --git a/tests/demo/some source.cpp b/tests/demo/some source.c
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
        );
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
        );
        assert!(files.is_empty());
    }

    const RENAMED_DIFF_WITH_CHANGES: &'static str = r#"diff --git a/tests/demo/some source.cpp b/tests/demo/some source.c
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
        );
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
        );
        assert!(!files.is_empty());
    }

    const BINARY_DIFF: &'static str = "diff --git a/some picture.png b/some picture.png\n\
                new file mode 100644\n\
                Binary files /dev/null and b/some picture.png differ\n";

    #[test]
    fn parse_binary_diff() {
        let files = parse_diff(
            BINARY_DIFF,
            &FileFilter::new(&[], &["png"], None),
            &LinesChangedOnly::Diff,
        );
        assert!(files.is_empty());
    }

    const TERSE_HEADERS: &'static str = r#"diff --git a/src/demo.cpp b/src/demo.cpp
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
        let files = parse_diff(TERSE_HEADERS, &file_filter, &LinesChangedOnly::Diff);
        let file_diff = files.get("src/demo.cpp").unwrap();
        assert_eq!(file_diff.diff_hunks, vec![3..4, 5..7, 17..19]);
    }
}
