//! Template fields: schema definition and validation.
//!
//! A `Field` is the authored schema element inside a template. A filled
//! value is stored on the task as a `serde_json::Value` keyed by the
//! field's id.
//!
//! v0.1 supports the following kinds (from `docs/TEMPLATES.md`):
//!
//! - `text`, `textarea` — strings
//! - `select` — single choice from `options`
//! - `multiselect` — subset of `options`
//! - `number` — numeric (integer-only if `integer: true`)
//! - `boolean` — bool
//! - `file_picker` — project-relative path string
//! - `glob` — glob pattern string
//! - `cmd_output` — captured at task-creation time (read-only value)
//! - `markdown_note` — documentation, no value
//! - `ref` — reference to another entity (v0.1 only `{ kind = "run", id }`)
//! - `component` — interactive component (v0.2 stub — accepts any JSON value)

mod validation;

pub use validation::{FieldValue, ValueType, validate_field};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Optional per-kind configuration (kept as a sub-table to keep the
/// default `Field` small and to make serde round-trips stable).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FieldOptions {
    pub options: Vec<OptionItem>,
    pub integer: bool,
    pub extensions: Vec<String>,
    pub root: Option<String>,
    pub base: Option<String>,
    pub cmd: Option<Vec<String>>,
    pub target: Option<String>,
    pub filter: Option<Value>,
    pub component_kind: Option<String>,
}

/// A select/multiselect option. Authors can write either a bare string
/// or a `{ id, label }` table; both deserialize cleanly.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OptionItem {
    Bare(String),
    Labeled { id: String, label: String },
}

impl OptionItem {
    pub fn id(&self) -> &str {
        match self {
            Self::Bare(s) => s,
            Self::Labeled { id, .. } => id,
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Self::Bare(s) => s,
            Self::Labeled { label, .. } => label,
        }
    }
}

/// Declarative validation rules. Which keys apply depends on `FieldKind`;
/// irrelevant keys are ignored.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Validation {
    pub min_length: Option<usize>,
    pub max_length: Option<usize>,
    pub regex: Option<String>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub allowed_values: Option<Vec<Value>>,
}

/// Every field kind recognized by v0.1. We keep this as a data-centric
/// enum with string tags so TOML serialization remains ergonomic:
/// `kind = "text"` rather than a wrapped structure per variant.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldKind {
    Text,
    Textarea,
    Select,
    Multiselect,
    Number,
    Boolean,
    FilePicker,
    Glob,
    CmdOutput,
    MarkdownNote,
    Ref,
    Component,
    #[serde(rename = "sequence-gram")]
    SequenceGram,
}

impl FieldKind {
    /// Does a value of this kind carry a user-facing value at all?
    /// `markdown_note` is documentation-only.
    pub fn has_value(self) -> bool {
        !matches!(self, Self::MarkdownNote)
    }
}

/// A single field declaration inside a template.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Field {
    pub id: String,
    pub kind: FieldKind,
    pub label: String,
    #[serde(default)]
    pub help: Option<String>,
    #[serde(default)]
    pub placeholder: Option<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub default: Option<Value>,
    #[serde(default)]
    pub show_if: Option<String>,
    #[serde(
        default,
        rename = "validation",
        skip_serializing_if = "is_default_validation"
    )]
    pub validation: Validation,
    #[serde(flatten, default)]
    pub options: FieldOptions,
}

fn is_default_validation(v: &Validation) -> bool {
    *v == Validation::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn option_item_parses_both_shapes() {
        // `untagged` works via the in-memory representation; test both
        // shapes through serde_json to avoid TOML's root-must-be-table rule.
        let bare: OptionItem = serde_json::from_str("\"alpha\"").unwrap();
        assert_eq!(bare.id(), "alpha");
        assert_eq!(bare.label(), "alpha");

        let labeled: OptionItem = serde_json::from_str(r#"{"id":"a","label":"Alpha"}"#).unwrap();
        assert_eq!(labeled.id(), "a");
        assert_eq!(labeled.label(), "Alpha");
    }

    #[test]
    fn option_item_parses_inside_template_toml() {
        // This is the shape users actually author: a TOML array of
        // options, either bare strings or labeled tables.
        #[derive(serde::Deserialize)]
        struct Holder {
            options: Vec<OptionItem>,
        }
        let src = r#"
            options = [
                "readability",
                { id = "perf", label = "Performance" },
            ]
        "#;
        let h: Holder = toml::from_str(src).unwrap();
        assert_eq!(h.options.len(), 2);
        assert_eq!(h.options[0].id(), "readability");
        assert_eq!(h.options[1].id(), "perf");
        assert_eq!(h.options[1].label(), "Performance");
    }

    #[test]
    fn field_deserializes_from_toml() {
        let src = r#"
            id = "module_path"
            kind = "file_picker"
            label = "Target module"
            required = true
            [validation]
            regex = "^src/.*\\.rs$"
        "#;
        let f: Field = toml::from_str(src).unwrap();
        assert_eq!(f.id, "module_path");
        assert_eq!(f.kind, FieldKind::FilePicker);
        assert!(f.required);
        assert_eq!(f.validation.regex.as_deref(), Some("^src/.*\\.rs$"));
    }

    #[test]
    fn field_roundtrips() {
        let src = r#"
            id = "goals"
            kind = "multiselect"
            label = "Refactor goals"
            required = true
            options = ["readability", "performance"]
        "#;
        let f: Field = toml::from_str(src).unwrap();
        let re = toml::to_string(&f).unwrap();
        let back: Field = toml::from_str(&re).unwrap();
        assert_eq!(f, back);
    }

    #[test]
    fn markdown_note_has_no_value() {
        assert!(!FieldKind::MarkdownNote.has_value());
        assert!(FieldKind::Text.has_value());
    }

    #[test]
    fn sequence_gram_deserializes_from_toml_tag() {
        let src = r#"
            id = "diagram"
            kind = "sequence-gram"
            label = "Sequence"
            required = true
        "#;
        let f: Field = toml::from_str(src).unwrap();
        assert_eq!(f.kind, FieldKind::SequenceGram);
    }
}
