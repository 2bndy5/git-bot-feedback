use pyo3::prelude::*;
mod wrapper;

#[pymodule]
mod git_bot_feedback {
    use std::collections::HashMap;

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

    #[pyfunction]
    pub fn parse_diff(
        diff: &str,
        file_filter: &FileFilter,
        lines_changed_only: LinesChangedOnly,
    ) -> PyResult<HashMap<String, FileDiffLines>> {
        let result = ::git_bot_feedback::parse_diff(diff, file_filter, &lines_changed_only)?;
        Ok(result)
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
