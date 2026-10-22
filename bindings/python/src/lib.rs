use pyo3::prelude::*;
mod wrapper;

#[doc = include_str!("../README.rst")]
#[pymodule]
mod git_bot_feedback {
    use std::{
        collections::HashMap,
        path::{Path, PathBuf},
    };

    use pyo3::prelude::*;

    #[pymodule_init]
    fn init(_m: &Bound<'_, PyModule>) -> PyResult<()> {
        pyo3_log::init();
        Ok(())
    }

    #[pymodule_export]
    use super::wrapper::GitClient;

    #[pymodule_export]
    use ::git_bot_feedback::OutputVariable;

    #[pymodule_export]
    use ::git_bot_feedback::AnnotationLevel;
    #[pymodule_export]
    use ::git_bot_feedback::FileAnnotation;

    #[pymodule_export]
    use ::git_bot_feedback::DiffHunkHeader;
    #[pymodule_export]
    use ::git_bot_feedback::FileDiffLines;
    #[pymodule_export]
    use ::git_bot_feedback::FileFilter;
    #[pymodule_export]
    use ::git_bot_feedback::LinesChangedOnly;

    /// A function to parse a diff string.
    ///
    /// Returns a mapping of file paths to their corresponding :py:class:`~git_bot_feedback.FileDiffLines`.
    #[pyfunction]
    #[pyo3(
        signature = (
            diff,
            file_filter,
            lines_changed_only = None,
        ),
        text_signature = "(diff: str, file_filter: FileFilter, lines_changed_only: LinesChangedOnly | None = None) -> dict[str, FileDiffLines]"
    )]
    pub fn parse_diff(
        diff: &str,
        file_filter: &FileFilter,
        lines_changed_only: Option<LinesChangedOnly>,
    ) -> PyResult<HashMap<String, FileDiffLines>> {
        let result = ::git_bot_feedback::parse_diff(
            diff,
            file_filter,
            &lines_changed_only.unwrap_or_default(),
        )?;
        Ok(result)
    }

    /// Find the closest folder, starting with ``path`` itself, that has a ``.git``
    /// folder or file (as in a git submodule).
    ///
    /// A relative ``path`` is resolved against the working directory.
    /// The returned path is absolute, but symbolic links are not resolved.
    /// Returns ``None`` if no such folder is found. This does not invoke the git CLI.
    #[pyfunction]
    #[pyo3(text_signature = "(path: str | os.PathLike[str]) -> Path | None")]
    pub fn nearest_git_root(path: PathBuf) -> Option<PathBuf> {
        ::git_bot_feedback::client::nearest_git_root(Path::new(&path))
    }

    #[pymodule_export]
    use ::git_bot_feedback::ReviewAction;
    #[pymodule_export]
    use ::git_bot_feedback::ReviewComment;
    #[pymodule_export]
    use ::git_bot_feedback::ReviewOptions;

    #[pymodule_export]
    use ::git_bot_feedback::CommentKind;
    #[pymodule_export]
    use ::git_bot_feedback::CommentPolicy;
    #[pymodule_export]
    use ::git_bot_feedback::ThreadCommentOptions;
}
