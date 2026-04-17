use git_bot_feedback::{
    OutputVariable, RestClientError,
    client::{RestApiClient, init_client},
};

struct ClientWrapper {
    client: Box<dyn RestApiClient + Send + Sync>,
}

#[tokio::main]
async fn main() -> Result<(), RestClientError> {
    let client_wrapper = ClientWrapper {
        client: init_client()?,
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
