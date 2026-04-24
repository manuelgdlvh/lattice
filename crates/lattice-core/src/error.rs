//! Core error types.
//!
//! The error surface is deliberately split into sub-enums per concern
//! (validation, rendering, derived resolution). They all roll up into
//! `CoreError` for convenient propagation at the library boundary.

use thiserror::Error;

/// Top-level error for anything returned from `lattice-core`.
#[derive(Debug, Error)]
pub enum CoreError {
    #[error(transparent)]
    Validation(#[from] ValidationError),
    #[error(transparent)]
    Render(#[from] RenderError),
    #[error(transparent)]
    Derive(#[from] DeriveError),
    #[error("invalid entity: {0}")]
    InvalidEntity(String),
}

/// A collection of one or more validation errors found on a single entity.
#[derive(Debug, Error)]
#[error("validation failed with {} error(s)", .errors.len())]
pub struct ValidationError {
    pub errors: Vec<FieldError>,
}

impl ValidationError {
    pub fn new(errors: Vec<FieldError>) -> Self {
        Self { errors }
    }

    pub fn single(e: FieldError) -> Self {
        Self { errors: vec![e] }
    }
}

/// A single validation failure, scoped to a specific field id.
#[derive(Debug, Clone, Error, PartialEq)]
#[error("[{field_id}] {kind}")]
pub struct FieldError {
    pub field_id: String,
    pub kind: FieldErrorKind,
}

impl FieldError {
    pub fn new(field_id: impl Into<String>, kind: FieldErrorKind) -> Self {
        Self {
            field_id: field_id.into(),
            kind,
        }
    }
}

/// Kinds of validation failures. Keep the variants terse and human-readable:
/// they surface directly in the UI.
#[derive(Debug, Clone, Error, PartialEq)]
pub enum FieldErrorKind {
    #[error("required field is missing")]
    Required,
    #[error("value is too short ({actual} < {min})")]
    TooShort { actual: usize, min: usize },
    #[error("value is too long ({actual} > {max})")]
    TooLong { actual: usize, max: usize },
    #[error("value is below minimum ({value} < {min})")]
    BelowMin { value: f64, min: f64 },
    #[error("value is above maximum ({value} > {max})")]
    AboveMax { value: f64, max: f64 },
    #[error("value does not match pattern `{pattern}`")]
    PatternMismatch { pattern: String },
    #[error("expected an integer, got a non-integer number")]
    NotInteger,
    #[error("value `{value}` is not in the allowed set")]
    NotAllowed { value: String },
    #[error("expected type {expected}, found {actual}")]
    WrongType {
        expected: &'static str,
        actual: &'static str,
    },
    #[error("{0}")]
    Custom(String),
}

/// Errors raised while rendering a template with `MiniJinja`.
#[derive(Debug, Error)]
pub enum RenderError {
    #[error("template engine error: {0}")]
    Engine(#[from] minijinja::Error),
    #[error("template refers to undefined variable `{0}`")]
    UndefinedVariable(String),
    #[error("invalid template body: {0}")]
    InvalidBody(String),
}

/// Errors returned by derived-value providers.
#[derive(Debug, Error)]
pub enum DeriveError {
    #[error("provider `{name}` failed: {reason}")]
    ProviderFailed { name: String, reason: String },
    #[error("provider `{name}` timed out after {timeout_ms} ms")]
    Timeout { name: String, timeout_ms: u64 },
    #[error("unknown provider kind `{0}`")]
    UnknownKind(String),
    #[error("io error: {0}")]
    Io(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_error_is_displayable() {
        let e = FieldError::new("name", FieldErrorKind::Required);
        assert_eq!(e.to_string(), "[name] required field is missing");
    }

    #[test]
    fn validation_error_wraps_many() {
        let v = ValidationError::new(vec![
            FieldError::new("a", FieldErrorKind::Required),
            FieldError::new("b", FieldErrorKind::TooShort { actual: 1, min: 5 }),
        ]);
        assert_eq!(v.errors.len(), 2);
        assert_eq!(v.to_string(), "validation failed with 2 error(s)");
    }
}
