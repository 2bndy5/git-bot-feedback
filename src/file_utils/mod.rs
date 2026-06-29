#[cfg(feature = "pyo3")]
use pyo3::prelude::*;

use std::ops::Range;

pub mod file_filter;
use crate::DiffHunkHeader;

/// An enum to help determine what constitutes a changed file based on the diff contents.
#[derive(PartialEq, Clone, Copy, Debug, Default)]
#[cfg_attr(docsrs, doc(cfg(feature = "file-changes")))]
#[cfg_attr(feature = "pyo3", pyclass(module = "git_bot_feedback", from_py_object))]
pub enum LinesChangedOnly {
    /// File is included regardless of changed lines in the diff.
    ///
    /// Use [`FileFilter`](crate::FileFilter) to filter files by
    /// extension and/or path.
    #[default]
    Off,

    /// Only include files with lines in the diff.
    ///
    /// Note, this *includes* files that only have lines with deletions.
    /// But, this *excludes* files that have no line changes at all
    /// (eg. renamed files with unmodified contents, or deleted files, or
    /// binary files).
    Diff,

    /// Only include files with lines in the diff that have additions.
    ///
    /// Note, this *excludes* files that only have lines with deletions.
    /// So, this is like [`LinesChangedOnly::Diff`] but stricter.
    On,
}

impl LinesChangedOnly {
    pub(crate) fn is_change_valid(&self, added_lines: bool, diff_hunks: bool) -> bool {
        match self {
            LinesChangedOnly::Off => true,
            LinesChangedOnly::Diff => diff_hunks,
            LinesChangedOnly::On => added_lines,
        }
    }
}

impl std::fmt::Display for LinesChangedOnly {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LinesChangedOnly::Off => write!(f, "false"),
            LinesChangedOnly::Diff => write!(f, "diff"),
            LinesChangedOnly::On => write!(f, "true"),
        }
    }
}

/// A structure to represent a file's changes per line numbers.
#[derive(Debug, Clone, Default)]
#[cfg_attr(docsrs, doc(cfg(feature = "file-changes")))]
#[cfg_attr(feature = "pyo3", pyclass(module = "git_bot_feedback", from_py_object))]
pub struct FileDiffLines {
    /// The list of lines numbers with additions.
    pub added_lines: Vec<u32>,

    /// The list of ranges that span only lines numbers with additions.
    ///
    /// The line numbers here disregard the old line numbers in the diff hunks.
    /// Each range describes the beginning and ending of a group of consecutive line numbers.
    pub added_ranges: Vec<Range<u32>>,

    /// The list of ranges that span the lines numbers present in diff chunks.
    ///
    /// The line numbers here disregard the old line numbers in the diff hunks.
    pub diff_hunks: Vec<Range<u32>>,
}

impl FileDiffLines {
    /// Instantiate an object with changed lines information.
    pub fn with_info(added_lines: Vec<u32>, diff_hunks: Vec<Range<u32>>) -> Self {
        let added_ranges = Self::consolidate_numbers_to_ranges(&added_lines);
        Self {
            added_lines,
            added_ranges,
            diff_hunks,
        }
    }

    /// A helper function to consolidate a [Vec<u32>] of line numbers into a
    /// [Vec<Range<u32>>] in which each range describes the beginning and
    /// ending of a group of consecutive line numbers.
    fn consolidate_numbers_to_ranges(lines: &[u32]) -> Vec<Range<u32>> {
        let mut iter_lines = lines.iter().enumerate();
        if let Some((_, start)) = iter_lines.next() {
            let mut range_start = *start;
            let mut ranges: Vec<Range<u32>> = Vec::new();
            let last_entry = lines.len() - 1;
            for (index, number) in iter_lines {
                if let Some(prev) = lines.get(index - 1)
                    && (number - 1) != *prev
                {
                    // non-consecutive number found
                    // push the previous range
                    ranges.push(range_start..(*prev + 1));
                    // and start a new range
                    // from the current number
                    range_start = *number;
                }
                if index == last_entry {
                    // last number
                    ranges.push(range_start..(*number + 1));
                }
            }
            ranges
        } else {
            Vec::new()
        }
    }

    /// Get the ranges of changed lines based on the `lines_changed_only` parameter.
    ///
    /// Use this to map [`Self::added_lines`] and [`Self::diff_hunks`] to a selection of
    /// [`LinesChangedOnly`] options.
    pub fn get_ranges(&self, lines_changed_only: &LinesChangedOnly) -> Option<Vec<Range<u32>>> {
        match lines_changed_only {
            LinesChangedOnly::Diff => Some(self.diff_hunks.to_vec()),
            LinesChangedOnly::On => Some(self.added_ranges.to_vec()),
            _ => None,
        }
    }

    /// Is the range from [`DiffHunkHeader`] contained in a single item of
    /// [`FileDiffLines::diff_hunks`]?
    pub fn is_hunk_in_diff(&self, hunk: &DiffHunkHeader) -> Option<(u32, u32)> {
        let (start_line, end_line) = if hunk.old_lines > 0 {
            // if old hunk's total lines is > 0
            let start = hunk.old_start;
            (start, start + hunk.old_lines)
        } else {
            // old hunk's total lines is 0, meaning changes were only added
            let start = hunk.new_start;
            // make old hunk's range span 1 line
            (start, start + 1)
        };
        let inclusive_end = end_line - 1;
        for range in &self.diff_hunks {
            if range.contains(&start_line) && range.contains(&inclusive_end) {
                return Some((start_line, end_line));
            }
        }
        None
    }

    /// Similar to [`FileDiffLines::is_hunk_in_diff()`] but looks for a single line instead of
    /// all lines in a [`DiffHunkHeader`].
    pub fn is_line_in_diff(&self, line: &u32) -> bool {
        for range in &self.diff_hunks {
            if range.contains(line) {
                return true;
            }
        }
        false
    }
}

#[cfg(feature = "pyo3")]
#[pymethods]
impl FileDiffLines {
    /// Create a new file diff lines instance.
    ///
    /// The ``added_ranges`` and ``diff_hunks`` are provided as
    /// tuples of ``(start, end)`` to represent ranges.
    #[new]
    #[pyo3(
        signature = (added_lines, added_ranges, diff_hunks),
        text_signature = "(added_lines: list[int], added_ranges: list[tuple[int, int]], diff_hunks: list[tuple[int, int]])"
    )]
    pub fn new_py(
        added_lines: Vec<u32>,
        added_ranges: Vec<(u32, u32)>,
        diff_hunks: Vec<(u32, u32)>,
    ) -> Self {
        Self {
            added_lines,
            added_ranges: added_ranges
                .into_iter()
                .map(|(start, end)| start..end)
                .collect(),
            diff_hunks: diff_hunks
                .into_iter()
                .map(|(start, end)| start..end)
                .collect(),
        }
    }

    /// Create a new file diff lines instance from given ``added_lines`` and ``diff_hunks``.
    ///
    /// This constructor is preferred because the ``added_ranges`` is automatically
    /// calculated from the ``added_lines``.
    #[staticmethod]
    #[pyo3(
        signature = (added_lines, diff_hunks),
        text_signature = "(added_lines: list[int], diff_hunks: list[tuple[int, int]]) -> FileDiffLines"
    )]
    pub fn from_info(added_lines: Vec<u32>, diff_hunks: Vec<(u32, u32)>) -> Self {
        Self::with_info(
            added_lines,
            diff_hunks
                .into_iter()
                .map(|(start, end)| start..end)
                .collect(),
        )
    }

    /// The range of line numbers whose lines were added.
    ///
    /// This takes the form of a list of tuples of
    /// ``(inclusive_start, exclusive_end)`` to represent ranges.
    #[getter]
    pub fn get_added_ranges(&self) -> Vec<(u32, u32)> {
        self.added_ranges
            .iter()
            .map(|range| (range.start, range.end))
            .collect()
    }

    /// The list of line numbers whose lines have additions.
    #[getter]
    pub fn get_added_lines(&self) -> Vec<u32> {
        self.added_lines.clone()
    }

    /// The range of line numbers that span the diff hunks.
    ///
    /// This takes the form of a list of tuples of
    /// ``(inclusive_start, exclusive_end)`` to represent ranges.
    #[getter]
    pub fn get_diff_hunks(&self) -> Vec<(u32, u32)> {
        self.diff_hunks
            .iter()
            .map(|range| (range.start, range.end))
            .collect()
    }

    /// Check if the given hunk header describes a hunk contained in the ``diff_hunks``.
    #[pyo3(
        name = "is_hunk_in_diff",
        signature = (hunk),
        text_signature = "(hunk: DiffHunkHeader) -> tuple[int, int] | None"
    )]
    pub fn is_hunk_in_diff_py(&self, hunk: &DiffHunkHeader) -> Option<(u32, u32)> {
        self.is_hunk_in_diff(hunk)
    }

    /// Check if the given line number is contained in the ``diff_hunks``.
    #[pyo3(
        name = "is_line_in_diff",
        signature = (line),
        text_signature = "(line: int) -> bool"
    )]
    pub fn is_line_in_diff_py(&self, line: u32) -> bool {
        self.is_line_in_diff(&line)
    }
}

#[cfg(test)]
mod test {
    #![allow(clippy::unwrap_used)]

    use super::{FileDiffLines, LinesChangedOnly};

    #[test]
    fn display_lines_changed_only() {
        assert_eq!(LinesChangedOnly::Off.to_string(), "false");
        assert_eq!(LinesChangedOnly::Diff.to_string(), "diff");
        assert_eq!(LinesChangedOnly::On.to_string(), "true");
    }

    #[test]
    fn get_ranges_none() {
        let file_obj = FileDiffLines::default();
        let ranges = file_obj.get_ranges(&LinesChangedOnly::Off);
        assert!(ranges.is_none());
    }

    #[test]
    fn get_ranges_diff() {
        #[allow(clippy::single_range_in_vec_init)]
        let diff_chunks = vec![1..11];
        let added_lines = vec![4, 5, 9];
        let file_obj = FileDiffLines::with_info(added_lines, diff_chunks.clone());
        let ranges = file_obj.get_ranges(&LinesChangedOnly::Diff);
        assert_eq!(ranges.unwrap(), diff_chunks);
    }

    #[test]
    fn get_ranges_added() {
        #[allow(clippy::single_range_in_vec_init)]
        let diff_chunks = vec![1..11];
        let added_lines = vec![4, 5, 9];
        let file_obj = FileDiffLines::with_info(added_lines, diff_chunks);
        let ranges = file_obj.get_ranges(&LinesChangedOnly::On);
        assert_eq!(ranges.unwrap(), vec![4..6, 9..10]);
    }

    #[test]
    fn line_not_in_diff() {
        let file_obj = FileDiffLines::default();
        assert!(!file_obj.is_line_in_diff(&42));
    }
}
