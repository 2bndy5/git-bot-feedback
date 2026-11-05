#[cfg(feature = "pyo3")]
use pyo3::prelude::*;
#[cfg(any(feature = "github", feature = "gitea"))]
use std::io::{self, Write};

#[cfg(any(feature = "github", feature = "gitea"))]
use crate::RestClientError;

/// A structure to describe the output of a file annotation.
#[derive(Debug, Default, Clone)]
#[cfg_attr(
    feature = "pyo3",
    pyclass(module = "git_bot_feedback", from_py_object, get_all, set_all)
)]
pub struct FileAnnotation {
    /// The severity level of the annotation.
    pub severity: AnnotationLevel,

    /// The path to the file being annotated.
    ///
    /// This is relative to the repository root.
    /// It should not start with a leading slash (not `./`).
    /// It should only use posix-style path separators (`/`), even on Windows runners.
    ///
    /// On Github, this can be left blank if the annotation is to be specific to the workflow run.
    pub path: String,

    /// The line number where the annotation starts (1-based).
    ///
    /// If not provided, the annotation will be scoped to the entire file (and [`Self::end_line`] will be ignored).
    ///
    /// This is ignored if [`Self::path`] is blank.
    pub start_line: Option<usize>,

    /// The line number where the annotation ends (1-based).
    ///
    /// If not provided, the annotation will be placed at the specified [`Self::start_line`] instead.
    ///
    /// This is ignored if
    ///
    /// - [`Self::path`] is blank.
    /// - [`Self::start_line`] is not provided.
    /// - [`Self::end_line`] is not greater than [`Self::start_line`].
    pub end_line: Option<usize>,

    /// The column number where the annotation starts (1-based).
    ///
    /// This is ignored if the [`Self::start_line`] is not provided,
    /// or if [`Self::path`] is blank.
    pub start_column: Option<usize>,

    /// The column number where the annotation ends (1-based).
    ///
    /// This is ignored if
    ///
    /// - the [`Self::path`] is blank
    /// - the [`Self::start_line`] is not provided
    /// - the [`Self::end_line`] is not greater than [`Self::start_line`]
    ///   and [`Self::start_column`] is provided
    ///   but is not less than this [`Self::end_column`]
    pub end_column: Option<usize>,

    /// The title of the annotation, which will be shown in the Git Server's UI.
    pub title: Option<String>,

    /// The message of the annotation, which will be shown in the Git Server's UI.
    ///
    /// This shall not contain any line breaks.
    /// Some Git Servers may support a limited set of markdown syntax, but this is not guaranteed.
    pub message: String,
}

#[cfg(feature = "pyo3")]
#[pymethods]
impl FileAnnotation {
    /// Create a new file annotation instance.
    #[new]
    #[allow(clippy::too_many_arguments)]
    #[pyo3(
        signature = (severity, path, message, start_line=None, end_line=None, start_column=None, end_column=None, title=None),
        text_signature = "(severity: AnnotationLevel, path: str, message: str, start_line: int | None = None, end_line: int | None = None, start_column: int | None = None, end_column: int | None = None, title: str | None = None)"
    )]
    pub fn new_py(
        severity: AnnotationLevel,
        path: String,
        message: String,
        start_line: Option<usize>,
        end_line: Option<usize>,
        start_column: Option<usize>,
        end_column: Option<usize>,
        title: Option<String>,
    ) -> Self {
        Self {
            severity,
            path,
            start_line,
            end_line,
            start_column,
            end_column,
            title,
            message,
        }
    }
}

/// The severity of a [`FileAnnotation`].
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "pyo3",
    pyclass(module = "git_bot_feedback", from_py_object, eq)
)]
pub enum AnnotationLevel {
    /// The annotation is for debugging purposes.
    Debug,
    /// The annotation is for informational purposes.
    #[default]
    Notice,
    /// The annotation is for warning purposes.
    Warning,
    /// The annotation is for error purposes.
    Error,
}

#[cfg(any(feature = "github", feature = "gitea"))]
#[cfg_attr(docsrs, doc(cfg(any(feature = "github", feature = "gitea"))))]
impl FileAnnotation {
    /// Format the [`FileAnnotation`] struct into the specific string format compatible with Github Actions.
    ///
    /// Gitea Actions also renders this format in the job's log.
    ///
    /// See [Github workflow commands documentation](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-commands#setting-a-debug-message).
    ///
    /// Example:
    /// ```text
    /// ::notice file={name},line={line},col={col},endLine={endLine},endColumn={endColumn},title={title}::{message}
    /// ```
    pub fn fmt_github(&self) -> String {
        let mut annotation_str = format!(
            "::{}",
            match self.severity {
                AnnotationLevel::Debug => "debug",
                AnnotationLevel::Notice => "notice",
                AnnotationLevel::Warning => "warning",
                AnnotationLevel::Error => "error",
            }
        );
        let file_path = self
            .path
            .replace("\\", "/")
            .trim_start()
            .trim_start_matches('/')
            .trim_start_matches("./")
            .trim()
            .to_string();
        if !file_path.is_empty() {
            annotation_str.push_str(" file=");
            annotation_str.push_str(file_path.as_str());
            if let Some(start_line) = self.start_line.map(|l| l.max(1)) {
                annotation_str.push_str(format!(",line={start_line}").as_str());
                let col = self.start_column.map(|c| c.max(1));
                if let Some(col) = col {
                    annotation_str.push_str(format!(",col={col}").as_str());
                }
                if let Some(end_line) = self.end_line.map(|l| l.max(1))
                    && end_line > start_line
                {
                    annotation_str.push_str(format!(",endline={end_line}").as_str());
                    if let Some(end_col) = self.end_column.map(|c| c.max(1)) {
                        annotation_str.push_str(format!(",endColumn={end_col}").as_str());
                    }
                } else if let Some(end_col) = self.end_column.map(|c| c.max(1))
                    && col.is_none_or(|c| c < end_col)
                {
                    annotation_str.push_str(format!(",endColumn={end_col}").as_str());
                }
            }
            if let Some(title) = &self.title {
                annotation_str.push_str(",title=");
                annotation_str.push_str(title.as_str());
            }
        } else if let Some(title) = &self.title {
            annotation_str.push_str(" title=");
            annotation_str.push_str(title.as_str());
        }
        format!("{annotation_str}::{}", self.message)
    }
}

/// Print the given `annotations` to stdout as workflow commands (see [`FileAnnotation::fmt_github`]).
///
/// This is how GitHub and Gitea runners receive file annotations.
#[cfg(any(feature = "github", feature = "gitea"))]
pub(crate) fn print_workflow_commands(
    annotations: &[FileAnnotation],
) -> Result<(), RestClientError> {
    if annotations.is_empty() {
        // Should probably be an error.
        // This check is only here to prevent needlessly locking stdout.
        return Ok(());
    }
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    for annotation in annotations {
        writeln!(&mut handle, "{}", annotation.fmt_github())
            .map_err(|e| RestClientError::io("write to file annotation to stdout", e))?;
    }
    handle
        .flush()
        .map_err(|e| RestClientError::io("flush stdout with file annotations", e))?;
    Ok(())
}

#[cfg(all(test, any(feature = "github", feature = "gitea")))]
mod tests {
    use super::{AnnotationLevel, FileAnnotation};

    #[test]
    fn generic_message() {
        let annotation = FileAnnotation {
            severity: AnnotationLevel::Debug,
            message: "This is a debug message".to_string(),
            ..Default::default()
        };
        assert_eq!(annotation.fmt_github(), "::debug::This is a debug message");
    }

    #[test]
    fn annotate_file() {
        let annotation = FileAnnotation {
            severity: AnnotationLevel::Warning,
            message: "This is a warning message".to_string(),
            path: "/.\\src\\main.rs".to_string(),
            title: Some("Warning Title".to_string()),
            ..Default::default()
        };
        assert_eq!(
            annotation.fmt_github(),
            "::warning file=src/main.rs,title=Warning Title::This is a warning message"
        );
    }

    #[test]
    fn annotate_file_with_start_line() {
        let annotation = FileAnnotation {
            severity: AnnotationLevel::Error,
            path: "src/lib.rs".to_string(),
            message: "This is an error message".to_string(),
            start_line: Some(10),
            ..Default::default()
        };
        assert_eq!(
            annotation.fmt_github(),
            "::error file=src/lib.rs,line=10::This is an error message"
        );
    }

    #[test]
    fn annotate_file_with_start_line_col() {
        let annotation = FileAnnotation {
            severity: AnnotationLevel::Error,
            path: "src/lib.rs".to_string(),
            message: "This is an error message".to_string(),
            start_line: Some(10),
            start_column: Some(5),
            ..Default::default()
        };
        assert_eq!(
            annotation.fmt_github(),
            "::error file=src/lib.rs,line=10,col=5::This is an error message"
        );
    }

    #[test]
    fn annotate_file_with_line_span() {
        let annotation = FileAnnotation {
            severity: AnnotationLevel::Notice,
            path: "src/lib.rs".to_string(),
            message: "This is a notice message".to_string(),
            start_line: Some(10),
            end_line: Some(20),
            ..Default::default()
        };
        assert_eq!(
            annotation.fmt_github(),
            "::notice file=src/lib.rs,line=10,endline=20::This is a notice message"
        );
    }

    #[test]
    fn annotate_file_with_line_col_span() {
        let annotation = FileAnnotation {
            severity: AnnotationLevel::Notice,
            path: "src/lib.rs".to_string(),
            message: "This is a notice message".to_string(),
            start_line: Some(10),
            start_column: Some(5),
            end_line: Some(20),
            end_column: Some(15),
            ..Default::default()
        };
        assert_eq!(
            annotation.fmt_github(),
            "::notice file=src/lib.rs,line=10,col=5,endline=20,endColumn=15::This is a notice message"
        );
    }

    #[test]
    fn annotate_file_with_col_span_on_1_line() {
        let annotation = FileAnnotation {
            severity: AnnotationLevel::Notice,
            path: "src/lib.rs".to_string(),
            message: "This is a notice message".to_string(),
            start_line: Some(10),
            end_line: Some(2),
            start_column: Some(5),
            end_column: Some(15),
            ..Default::default()
        };
        assert_eq!(
            annotation.fmt_github(),
            "::notice file=src/lib.rs,line=10,col=5,endColumn=15::This is a notice message"
        );
    }

    #[test]
    fn annotate_blank_path_with_title() {
        let annotation = FileAnnotation {
            severity: AnnotationLevel::Debug,
            message: "This is a debug message".to_string(),
            title: Some("Debug Title".to_string()),
            start_line: Some(10),
            ..Default::default()
        };
        assert_eq!(
            annotation.fmt_github(),
            "::debug title=Debug Title::This is a debug message"
        );
    }

    #[test]
    fn annotate_blank_path_no_title() {
        let annotation = FileAnnotation {
            severity: AnnotationLevel::Debug,
            message: "This is a debug message".to_string(),
            start_line: Some(10),
            ..Default::default()
        };
        assert_eq!(annotation.fmt_github(), "::debug::This is a debug message");
    }
}
