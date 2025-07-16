use git_bot_feedback::{OutputVariable, RestApiClient, RestClientError, client::GithubApiClient};
use std::{env, io::Read, path::Path};
use tempfile::{NamedTempFile, tempdir};
mod common;
use common::logger_init;

#[derive(Debug, Default)]
struct TestParams {
    fail_file: bool,
    absent: bool,
    bad_var: bool,
    empty_pairs: bool,
}

const VAR_NAME: &str = "STEP_OUTPUT_VAR";
const VAR_VALUE: &str = "some data";

async fn append_output_vars(test_params: TestParams) -> String {
    let tmp_dir = tempdir().unwrap();
    logger_init();
    log::set_max_level(log::LevelFilter::Debug);
    let mut out_var_path = NamedTempFile::new_in(tmp_dir.path()).unwrap();
    if test_params.absent {
        unsafe {
            env::remove_var("GITHUB_OUTPUT");
        }
    } else {
        unsafe {
            env::set_var(
                "GITHUB_OUTPUT",
                if test_params.fail_file {
                    Path::new("not-a-file.txt")
                } else {
                    out_var_path.path()
                },
            );
        }
    }

    let out_vars = if test_params.bad_var {
        [OutputVariable {
            name: VAR_NAME.to_string(),
            value: "bad\nvalue".to_string(),
        }]
    } else {
        [OutputVariable {
            name: VAR_NAME.to_string(),
            value: VAR_VALUE.to_string(),
        }]
    };
    let mut out_vars_content = String::new();
    match GithubApiClient::write_output_variables(if test_params.empty_pairs {
        &[]
    } else {
        &out_vars
    }) {
        Ok(_) => {
            out_var_path.read_to_string(&mut out_vars_content).unwrap();
        }
        Err(e) => {
            eprintln!("Encountered error: {e}");
            if test_params.fail_file {
                assert!(matches!(e, RestClientError::Io(_)));
            } else if test_params.bad_var {
                assert!(matches!(e, RestClientError::OutputVarError(_)));
            } else {
                panic!("Unexpected failure to write to GITHUB_OUTPUT");
            }
        }
    }
    out_vars_content
}

#[tokio::test]
async fn fail_gh_out() {
    let out = append_output_vars(TestParams {
        fail_file: true,
        ..Default::default()
    })
    .await;
    assert!(out.is_empty());
}

#[tokio::test]
async fn pass_gh_out() {
    let out = append_output_vars(TestParams::default()).await;
    assert!(out.contains(format!("{VAR_NAME}={VAR_VALUE}\n").as_str()));
}

#[tokio::test]
async fn absent_gh_out() {
    let out = append_output_vars(TestParams {
        absent: true,
        ..Default::default()
    })
    .await;
    assert!(out.is_empty());
}

#[tokio::test]
async fn bad_var_val() {
    let out = append_output_vars(TestParams {
        bad_var: true,
        ..Default::default()
    })
    .await;
    assert!(out.is_empty());
}

#[tokio::test]
async fn empty_pairs() {
    let out = append_output_vars(TestParams {
        empty_pairs: true,
        ..Default::default()
    })
    .await;
    assert!(out.is_empty());
}
