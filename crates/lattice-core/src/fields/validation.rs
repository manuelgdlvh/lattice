//! Value validation for a single field.
//!
//! Validation is split from the surrounding entity validation
//! (`crate::validation`) so we can unit-test each field kind in
//! isolation.

use regex::Regex;
use serde_json::Value;

use crate::error::{FieldError, FieldErrorKind};
use crate::fields::{Field, FieldKind};

/// A strongly-typed wrapper around a JSON value, tagged with what the
/// schema *expects*. Used to produce good error messages when a TOML
/// author gave us, e.g., a string where a number was expected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueType {
    String,
    Number,
    Bool,
    Array,
    Object,
    Null,
}

impl ValueType {
    fn of(v: &Value) -> Self {
        match v {
            Value::Null => Self::Null,
            Value::Bool(_) => Self::Bool,
            Value::Number(_) => Self::Number,
            Value::String(_) => Self::String,
            Value::Array(_) => Self::Array,
            Value::Object(_) => Self::Object,
        }
    }

    fn as_static(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Number => "number",
            Self::Bool => "boolean",
            Self::Array => "array",
            Self::Object => "object",
            Self::Null => "null",
        }
    }
}

/// A helper for callers that want to talk about values generically.
pub type FieldValue = Value;

/// Validate a single field value against its declared schema.
///
/// Returns all errors for this field (not just the first) so the UI can
/// display them together.
pub fn validate_field(field: &Field, value: Option<&Value>) -> Vec<FieldError> {
    let mut out = Vec::new();

    if !field.kind.has_value() {
        return out;
    }

    let v = match value {
        None | Some(Value::Null) => {
            if field.required {
                out.push(FieldError::new(&field.id, FieldErrorKind::Required));
            }
            return out;
        }
        Some(v) => v,
    };

    match field.kind {
        FieldKind::Textarea
        | FieldKind::SequenceGram
        | FieldKind::CodeBlocks
        | FieldKind::Gherkin
        | FieldKind::OpenApi => {
            validate_string(&field.id, &field.validation, v, &mut out);
        }
        FieldKind::Select => validate_select(field, v, &mut out),
        FieldKind::Multiselect => validate_multiselect(field, v, &mut out),
    }

    if let Some(allowed) = field.validation.allowed_values.as_ref()
        && !allowed.iter().any(|a| a == v)
    {
        out.push(FieldError::new(
            &field.id,
            FieldErrorKind::NotAllowed {
                value: short_value_display(v),
            },
        ));
    }

    out
}

fn validate_string(
    field_id: &str,
    rules: &super::Validation,
    v: &Value,
    out: &mut Vec<FieldError>,
) {
    let Some(s) = v.as_str() else {
        out.push(FieldError::new(
            field_id,
            FieldErrorKind::WrongType {
                expected: "string",
                actual: ValueType::of(v).as_static(),
            },
        ));
        return;
    };

    if let Some(min) = rules.min_length
        && s.chars().count() < min
    {
        out.push(FieldError::new(
            field_id,
            FieldErrorKind::TooShort {
                actual: s.chars().count(),
                min,
            },
        ));
    }
    if let Some(max) = rules.max_length
        && s.chars().count() > max
    {
        out.push(FieldError::new(
            field_id,
            FieldErrorKind::TooLong {
                actual: s.chars().count(),
                max,
            },
        ));
    }

    if let Some(pattern) = rules.regex.as_deref() {
        match Regex::new(pattern) {
            Ok(re) if !re.is_match(s) => {
                out.push(FieldError::new(
                    field_id,
                    FieldErrorKind::PatternMismatch {
                        pattern: pattern.to_string(),
                    },
                ));
            }
            Err(e) => {
                out.push(FieldError::new(
                    field_id,
                    FieldErrorKind::Custom(format!("invalid regex `{pattern}`: {e}")),
                ));
            }
            _ => {}
        }
    }
}

fn validate_select(field: &Field, v: &Value, out: &mut Vec<FieldError>) {
    let Some(s) = v.as_str() else {
        out.push(FieldError::new(
            &field.id,
            FieldErrorKind::WrongType {
                expected: "string",
                actual: ValueType::of(v).as_static(),
            },
        ));
        return;
    };
    if !field.options.options.iter().any(|o| o.id() == s) {
        out.push(FieldError::new(
            &field.id,
            FieldErrorKind::NotAllowed {
                value: s.to_string(),
            },
        ));
    }
}

fn validate_multiselect(field: &Field, v: &Value, out: &mut Vec<FieldError>) {
    let Some(arr) = v.as_array() else {
        out.push(FieldError::new(
            &field.id,
            FieldErrorKind::WrongType {
                expected: "array",
                actual: ValueType::of(v).as_static(),
            },
        ));
        return;
    };
    for item in arr {
        let Some(s) = item.as_str() else {
            out.push(FieldError::new(
                &field.id,
                FieldErrorKind::WrongType {
                    expected: "string",
                    actual: ValueType::of(item).as_static(),
                },
            ));
            continue;
        };
        if !field.options.options.iter().any(|o| o.id() == s) {
            out.push(FieldError::new(
                &field.id,
                FieldErrorKind::NotAllowed {
                    value: s.to_string(),
                },
            ));
        }
    }
}

fn short_value_display(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fields::{FieldOptions, OptionItem, Validation};
    use serde_json::json;

    fn textarea_field(required: bool) -> Field {
        Field {
            id: "name".into(),
            kind: FieldKind::Textarea,
            label: "Name".into(),
            help: None,
            placeholder: None,
            required,
            default: None,
            show_if: None,
            validation: Validation {
                min_length: Some(2),
                max_length: Some(5),
                ..Validation::default()
            },
            options: FieldOptions::default(),
        }
    }

    #[test]
    fn required_missing_is_error() {
        let f = textarea_field(true);
        let errs = validate_field(&f, None);
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].kind, FieldErrorKind::Required);
    }

    #[test]
    fn optional_missing_is_ok() {
        let f = textarea_field(false);
        assert!(validate_field(&f, None).is_empty());
    }

    #[test]
    fn string_length_bounds() {
        let f = textarea_field(true);
        assert!(!validate_field(&f, Some(&json!("a"))).is_empty());
        assert!(validate_field(&f, Some(&json!("abcd"))).is_empty());
        assert!(!validate_field(&f, Some(&json!("abcdef"))).is_empty());
    }

    #[test]
    fn select_rejects_unknown_option() {
        let mut f = textarea_field(true);
        f.kind = FieldKind::Select;
        f.validation = Validation::default();
        f.options.options = vec![OptionItem::Bare("a".into()), OptionItem::Bare("b".into())];
        assert!(validate_field(&f, Some(&json!("a"))).is_empty());
        let errs = validate_field(&f, Some(&json!("c")));
        assert_eq!(errs.len(), 1);
        assert!(matches!(errs[0].kind, FieldErrorKind::NotAllowed { .. }));
    }

    #[test]
    fn multiselect_flags_each_bad_item() {
        let f = Field {
            id: "tags".into(),
            kind: FieldKind::Multiselect,
            label: "Tags".into(),
            help: None,
            placeholder: None,
            required: true,
            default: None,
            show_if: None,
            validation: Validation::default(),
            options: FieldOptions {
                options: vec![OptionItem::Bare("a".into()), OptionItem::Bare("b".into())],
                ..FieldOptions::default()
            },
        };
        let errs = validate_field(&f, Some(&json!(["a", "c", "d"])));
        assert_eq!(errs.len(), 2);
    }

    #[test]
    fn regex_pattern_applies() {
        let mut f = textarea_field(true);
        f.validation = Validation {
            min_length: None,
            max_length: None,
            regex: Some("^[A-Z]+-[0-9]+$".into()),
            ..Validation::default()
        };
        assert!(validate_field(&f, Some(&json!("PROJ-123"))).is_empty());
        let errs = validate_field(&f, Some(&json!("lowercase-9")));
        assert_eq!(errs.len(), 1);
        assert!(matches!(
            errs[0].kind,
            FieldErrorKind::PatternMismatch { .. }
        ));
    }
}
