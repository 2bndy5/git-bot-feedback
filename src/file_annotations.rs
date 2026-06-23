#[cfg(feature = "pyo3")]
use pyo3::prelude::*;

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
    /// - [`Self::path`] is blank.
    /// - [`Self::start_line`] is not provided.
    /// - [`Self::end_line`] is not greater than [`Self::start_line`].
    pub end_line: Option<usize>,

    /// The column number where the annotation starts (1-based).
    ///
    /// This is ignored if the [`Self::start_line`] is not provided, or if [`Self::path`] is blank.
    pub start_column: Option<usize>,

    /// The column number where the annotation ends (1-based).
    ///
    /// This is ignored if
    /// - the [`Self::path`] is blank
    /// - the [`Self::start_line`] is not provided
    /// - the [`Self::end_line`] is not greater than to [`Self::start_line`]
    ///   and [`Self::start_column`] is provided but is not less than this [`Self::end_column`]
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
