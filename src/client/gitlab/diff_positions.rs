//! This submodule maps lines in a merge request's diff to the positions GitLab expects.
//!
//! GitLab anchors a comment on a line of the diff using both the line number in the
//! new version of the file (`new_line`) and the line number in the old version of
//! the file (`old_line`). An added line only has a `new_line`, but an unchanged line
//! needs both. Otherwise, GitLab cannot find the line in the diff.

use std::collections::HashMap;

use crate::{
    error::DiffError,
    git_diff::{DiffHunkHeader, split_diff},
};

/// A hunk of a file's diff.
#[derive(Debug, PartialEq, Eq)]
struct Hunk {
    header: DiffHunkHeader,
    /// Maps each line number (in the new version) to the line number in the old version.
    ///
    /// The old line number is [`None`] for added lines.
    lines: HashMap<u32, Option<u32>>,
}

impl Hunk {
    /// Map the lines of a hunk.
    ///
    /// The `text` is everything after the hunk's header, starting with the rest of the header's line.
    fn new(header: DiffHunkHeader, text: &str) -> Self {
        let mut lines = HashMap::new();
        let (mut old_line, mut new_line) = (header.old_start, header.new_start);
        let (mut old_left, mut new_left) = (header.old_lines, header.new_lines);
        // The text ends with the line break of the hunk's last line.
        let text = text.strip_suffix('\n').unwrap_or(text);
        for line in text.split('\n').skip(1) {
            if old_left == 0 && new_left == 0 {
                break; // the header's line counts were wrong
            }
            match line.chars().next() {
                Some('+') => {
                    lines.insert(new_line, None);
                    new_line += 1;
                    new_left = new_left.saturating_sub(1);
                }
                Some('-') => {
                    old_line += 1;
                    old_left = old_left.saturating_sub(1);
                }
                Some('\\') => {} // "\ No newline at end of file"
                _ => {
                    lines.insert(new_line, Some(old_line));
                    new_line += 1;
                    old_line += 1;
                    old_left = old_left.saturating_sub(1);
                    new_left = new_left.saturating_sub(1);
                }
            }
        }
        Self { header, lines }
    }

    /// The first line number (in the new version) of this hunk.
    fn new_first(&self) -> u32 {
        // An empty range (count is 0) refers to the line *before* the change.
        self.header.new_start + u32::from(self.header.new_lines == 0)
    }

    /// The first line number (in the new version) after this hunk.
    fn new_end(&self) -> u32 {
        self.new_first() + self.header.new_lines
    }

    /// The first line number (in the old version) after this hunk.
    fn old_end(&self) -> u32 {
        self.header.old_start + u32::from(self.header.old_lines == 0) + self.header.old_lines
    }
}

/// The diff information about a single file needed to anchor a comment on one of its lines.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct FilePositions {
    /// The file's path in the old version.
    pub old_path: String,
    hunks: Vec<Hunk>,
}

impl FilePositions {
    /// Get the line number in the old version of the file that corresponds
    /// to the given line number in the new version.
    ///
    /// Returns [`None`] if the given line was added.
    pub fn old_line(&self, new_line: u32) -> Option<u32> {
        let mut offset = 0i64;
        for hunk in &self.hunks {
            if new_line < hunk.new_first() {
                break;
            }
            if new_line < hunk.new_end() {
                return hunk.lines.get(&new_line).copied().flatten();
            }
            offset = hunk.old_end() as i64 - hunk.new_end() as i64;
        }
        // an unchanged line outside of the diff's hunks
        u32::try_from(new_line as i64 + offset).ok()
    }
}

/// Parse a unified diff (as output by `git diff`) into a map of file names to their [`FilePositions`].
///
/// Deleted files are excluded because there are no lines to comment on.
pub fn parse_positions(diff: &str) -> Result<HashMap<String, FilePositions>, DiffError> {
    let mut files = HashMap::new();
    for file in split_diff(diff)? {
        let mut old_path = None;
        let mut deleted = false;
        for line in file.front_matter.lines() {
            if line == "+++ /dev/null" {
                deleted = true;
            } else if let Some(path) = line
                .strip_prefix("--- a/")
                .or_else(|| line.strip_prefix("rename from "))
            {
                old_path = Some(path.strip_prefix('/').unwrap_or(path));
            }
        }
        if deleted {
            continue;
        }
        let positions = FilePositions {
            old_path: old_path.unwrap_or(file.name).to_string(),
            hunks: file
                .hunks
                .into_iter()
                .map(|(header, text)| Hunk::new(header, text))
                .collect(),
        };
        files.entry(file.name.to_string()).or_insert(positions);
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::parse_positions;

    const DIFF: &str = "diff --git a/src/demo.cpp b/src/demo.cpp
index 1234567..89abcde 100644
--- a/src/demo.cpp
+++ b/src/demo.cpp
@@ -2,4 +2,5 @@
 #include <stdio.h>
-#include \"math.h\"
+#include <math.h>
+#include <string>

 int main() {
@@ -20,2 +20,0 @@ int main() {
-    // removed
-    // lines
diff --git a/src/new.cpp b/src/new.cpp
new file mode 100644
index 0000000..1234567
--- /dev/null
+++ b/src/new.cpp
@@ -0,0 +1,2 @@
+int x = 0;
+int y = 1;
\\ No newline at end of file
diff --git a/src/old.cpp b/src/old.cpp
deleted file mode 100644
index 1234567..0000000
--- a/src/old.cpp
+++ /dev/null
@@ -1 +0,0 @@
-int z = 2;
diff --git a/src/before.h b/src/after.h
similarity index 100%
rename from src/before.h
rename to src/after.h
";

    #[test]
    fn modified_file() {
        let files = parse_positions(DIFF).unwrap();
        let file = files.get("src/demo.cpp").unwrap();
        assert_eq!(file.old_path, "src/demo.cpp");
        // unchanged line before the first hunk
        assert_eq!(file.old_line(1), Some(1));
        // context line in a hunk
        assert_eq!(file.old_line(2), Some(2));
        // added lines
        assert_eq!(file.old_line(3), None);
        assert_eq!(file.old_line(4), None);
        // context lines after the additions
        assert_eq!(file.old_line(5), Some(4));
        assert_eq!(file.old_line(6), Some(5));
        // unchanged lines between hunks and after the last hunk
        assert_eq!(file.old_line(7), Some(6));
        assert_eq!(file.old_line(20), Some(19));
        // unchanged line after deleted lines
        assert_eq!(file.old_line(21), Some(22));
    }

    #[test]
    fn new_file() {
        let files = parse_positions(DIFF).unwrap();
        let file = files.get("src/new.cpp").unwrap();
        assert_eq!(file.old_path, "src/new.cpp");
        assert_eq!(file.old_line(1), None);
        assert_eq!(file.old_line(2), None);
    }

    #[test]
    fn deleted_file() {
        let files = parse_positions(DIFF).unwrap();
        assert!(!files.contains_key("src/old.cpp"));
        assert!(!files.contains_key("dev/null"));
    }

    #[test]
    fn renamed_file() {
        let files = parse_positions(DIFF).unwrap();
        let file = files.get("src/after.h").unwrap();
        assert_eq!(file.old_path, "src/before.h");
        assert_eq!(file.old_line(10), Some(10));
    }

    #[test]
    fn pure_addition_hunk() {
        let diff = "diff --git a/a.c b/a.c\n--- a/a.c\n+++ b/a.c\n@@ -4,0 +5,2 @@\n+one\n+two\n";
        let files = parse_positions(diff).unwrap();
        let file = files.get("a.c").unwrap();
        assert_eq!(file.old_line(4), Some(4));
        assert_eq!(file.old_line(5), None);
        assert_eq!(file.old_line(6), None);
        assert_eq!(file.old_line(7), Some(5));
    }

    #[test]
    fn no_newline_at_end_of_file() {
        let diff = "diff --git a/a.c b/a.c\n--- a/a.c\n+++ b/a.c\n@@ -1 +1 @@\n-old\n\\ No newline at end of file\n+new\n";
        let files = parse_positions(diff).unwrap();
        let file = files.get("a.c").unwrap();
        assert_eq!(file.old_line(1), None);
        assert_eq!(file.old_line(2), Some(2));
    }

    #[test]
    fn hunk_with_wrong_line_counts() {
        // The first hunk claims more lines than it has.
        let diff = "diff --git a/a.c b/a.c\n--- a/a.c\n+++ b/a.c\n@@ -1,5 +1,6 @@\n+new\n context\ndiff --git a/b.c b/b.c\n--- a/b.c\n+++ b/b.c\n@@ -0,0 +1 @@\n+new\n";
        let files = parse_positions(diff).unwrap();
        assert_eq!(files.get("a.c").unwrap().old_line(1), None);
        assert_eq!(files.get("a.c").unwrap().old_line(2), Some(1));
        assert_eq!(files.get("b.c").unwrap().old_line(1), None);
    }

    #[test]
    fn truncated_diff() {
        // The last hunk claims more lines than the diff has.
        let diff =
            "diff --git a/a.c b/a.c\n--- a/a.c\n+++ b/a.c\n@@ -1,3 +1,4 @@\n context\n+new\n";
        let files = parse_positions(diff).unwrap();
        let file = files.get("a.c").unwrap();
        assert_eq!(file.old_line(1), Some(1));
        assert_eq!(file.old_line(2), None);
    }

    #[test]
    fn malformed_hunk_header() {
        let diff = "diff --git a/a.c b/a.c\n--- a/a.c\n+++ b/a.c\n@@ -x +y @@\n+one\n";
        let files = parse_positions(diff).unwrap();
        let file = files.get("a.c").unwrap();
        // no hunks were parsed, so all lines are considered unchanged
        assert_eq!(file.old_line(1), Some(1));
    }

    #[test]
    fn hunk_header_in_content() {
        let diff = "diff --git a/a.c b/a.c\n--- a/a.c\n+++ b/a.c\n@@ -1,2 +1,3 @@\n+// @@ -9 +9 @@\n one\n two\n";
        let files = parse_positions(diff).unwrap();
        let file = files.get("a.c").unwrap();
        assert_eq!(file.old_line(1), None);
        assert_eq!(file.old_line(2), Some(1));
        assert_eq!(file.old_line(3), Some(2));
        assert_eq!(file.old_line(4), Some(3));
    }

    #[test]
    fn malformed_diff() {
        assert!(parse_positions("").is_ok_and(|files| files.is_empty()));
        assert!(parse_positions("not a diff").is_err());
        assert!(parse_positions("diff --git a/a.c b/a.c\nnonsense\n").is_err());
    }
}
