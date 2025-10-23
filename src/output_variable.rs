use std::fmt::Display;

/// A type to represent an output variable.
///
/// This is akin to the key/value pairs used in most
/// config file formats but with some limitations:
///
/// - Both [OutputVariable::name] and [OutputVariable::value] must be UTF-8 encoded.
/// - The [OutputVariable::value] cannot span multiple lines.
#[derive(Debug, Clone)]
pub struct OutputVariable {
    /// The output variable's name.
    pub name: String,

    /// The output variable's value.
    pub value: String,
}

impl OutputVariable {
    /// Validate that the output variable is well-formed.
    ///
    /// Typically only used by implementations of
    /// [`RestApiClient::write_output_variables`](crate::client::RestApiClient::write_output_variables).
    pub fn validate(&self) -> bool {
        !self.value.contains("\n")
    }
}

impl Display for OutputVariable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}={}", self.name, self.value)
    }
}
