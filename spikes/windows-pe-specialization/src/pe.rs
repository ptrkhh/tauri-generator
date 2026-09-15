use serde_json::Value;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeError {
    InvalidSpec(String),
    InvalidText(String),
}

impl fmt::Display for PeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSpec(message) | Self::InvalidText(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for PeError {}

pub fn validate_text(value: &str, field: &str) -> Result<(), PeError> {
    if value.is_empty() || value.chars().any(char::is_control) || value.len() > 255 {
        return Err(PeError::InvalidText(format!("invalid {field}")));
    }
    Ok(())
}

pub fn validate_spec(bytes: &[u8]) -> Result<Vec<u8>, PeError> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|error| PeError::InvalidSpec(error.to_string()))?;
    let object = value
        .as_object()
        .ok_or_else(|| PeError::InvalidSpec("spec must be a JSON object".into()))?;
    if !object
        .get("schemaVersion")
        .is_some_and(|value| value.as_i64().is_some())
    {
        return Err(PeError::InvalidSpec(
            "schemaVersion must be an integer".into(),
        ));
    }
    for field in ["appId", "displayName", "slug", "launchUrl"] {
        let value = object
            .get(field)
            .and_then(Value::as_str)
            .ok_or_else(|| PeError::InvalidSpec(format!("{field} must be a string")))?;
        validate_text(value, field)
            .map_err(|_| PeError::InvalidSpec(format!("invalid {field}")))?;
    }
    Ok(bytes.to_vec())
}
