use git_bot_feedback::{
    OutputVariable, RestClientError,
    client::{GithubApiClient, RestApiClient},
};

struct ClientWrapper {
    client: Box<dyn RestApiClient>,
}

#[tokio::main]
async fn main() -> Result<(), RestClientError> {
    let client_wrapper = ClientWrapper {
        client: Box::new(GithubApiClient::new()?),
    };
    let output_var = OutputVariable {
        name: "STEP_OUTPUT_VAR".to_string(),
        value: "some data".to_string(),
    };
    client_wrapper
        .client
        .write_output_variables(&[output_var])?;
    Ok(())
}
