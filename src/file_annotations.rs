/// A structure to describe the output of a file annotation.
#[derive(Debug, Default)]
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
    /// This is ignored if [`Self::path`] is blank.
    pub end_line: Option<usize>,

    /// The column number where the annotation starts (1-based).
    ///
    /// This is ignored if the [`Self::start_line`] is not provided, or if [`Self::path`] is blank.
    pub start_column: Option<usize>,

    /// The column number where the annotation ends (1-based).
    ///
    /// This is ignored if
    /// - the [`Self::start_line`] and [`Self::end_line`] are not provided
    /// - the [`Self::end_line`] is less than or equal to [`Self::start_line`]
    /// - the [`Self::start_column`] is provided but is not less than this [`Self::end_column`]
    /// - the [`Self::path`] is blank
    pub end_column: Option<usize>,

    /// The title of the annotation, which will be shown in the Git Server's UI.
    pub title: Option<String>,

    /// The message of the annotation, which will be shown in the Git Server's UI.
    ///
    /// This shall not contain any line breaks.
    /// Some Git Servers may support a limited set of markdown syntax, but this is not guaranteed.
    pub message: String,
}

#[derive(Debug, Default)]
pub enum AnnotationLevel {
    Debug,
    #[default]
    Notice,
    Warning,
    Error,
}
