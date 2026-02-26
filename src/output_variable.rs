use std::fmt::Display;

use crate::error::OutputVariableError;

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
    pub fn validate(&self) -> Result<(), OutputVariableError> {
        let name = self.name.trim();
        if name.is_empty() {
            return Err(OutputVariableError::NameIsEmpty);
        }
        for (i, c) in name.chars().enumerate() {
            if i == 0 && c.is_ascii_digit() {
                return Err(OutputVariableError::NameStartsWithNumber(name.to_string()));
            }
            if !(c.is_ascii_alphanumeric() || c == '_' || c == '-') {
                return Err(OutputVariableError::NameContainsNonPrintableCharacters(
                    name.to_string(),
                ));
            }
        }
        let value = self.value.trim();
        if !value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c.is_ascii_punctuation() || !c.is_ascii_control())
        {
            return Err(OutputVariableError::ValueContainsNonPrintableCharacters(
                value.to_string(),
            ));
        }
        Ok(())
    }
}

impl Display for OutputVariable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}={}", self.name, self.value)
    }
}

#[cfg(test)]
mod tests {
    use super::{OutputVariable, OutputVariableError};

    #[test]
    fn empty_name() {
        let var = OutputVariable {
            name: "   ".to_string(),
            value: "value".to_string(),
        };
        assert_eq!(var.validate(), Err(OutputVariableError::NameIsEmpty));
    }

    #[test]
    fn name_starts_with_number() {
        let var = OutputVariable {
            name: "1var".to_string(),
            value: "value".to_string(),
        };
        assert_eq!(
            var.validate(),
            Err(OutputVariableError::NameStartsWithNumber(
                "1var".to_string()
            ))
        );
    }

    #[test]
    fn name_contains_non_printable_characters() {
        let var = OutputVariable {
            name: "var\nname".to_string(),
            value: "value".to_string(),
        };
        assert_eq!(
            var.validate(),
            Err(OutputVariableError::NameContainsNonPrintableCharacters(
                "var\nname".to_string()
            ))
        );
    }

    #[test]
    fn value_contains_non_printable_characters() {
        let var = OutputVariable {
            name: "var".to_string(),
            value: "(val)\nline2".to_string(),
        };
        assert_eq!(
            var.validate(),
            Err(OutputVariableError::ValueContainsNonPrintableCharacters(
                "(val)\nline2".to_string()
            ))
        );
    }

    #[test]
    fn valid_variable() {
        OutputVariable {
            name: " VAR_NAME ".to_string(),
            value: " value -(123) ".to_string(),
        }
        .validate()
        .unwrap();
    }
}
