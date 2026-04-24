//! `Template` entity — the schema + preamble + prompt body + derived
//! spec that tasks are instantiated from.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::fields::Field;
use crate::ids::TemplateId;
use crate::time::Timestamp;

use super::CURRENT_SCHEMA_VERSION;

fn current_schema_version() -> u32 {
    CURRENT_SCHEMA_VERSION
}

/// Static markdown prefix for the prompt. Tiny wrapper so the TOML shape
/// matches the spec (`[preamble] markdown = "..."`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preamble {
    pub markdown: String,
}

/// A derived-value declaration. The concrete resolver lives in the
/// `derived` module; this type is purely the schema as authored.
///
/// We carry the raw JSON value so we can evolve providers without
/// breaking the TOML shape, and so we can lean on the `DerivedProvider`
/// trait for strongly-typed parsing at resolution time.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DerivedSpec(pub Value);

/// Grouped-field declaration. Purely presentational.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FieldGroup {
    pub title: String,
    #[serde(default)]
    pub help: Option<String>,
    pub fields: Vec<String>,
}

/// Prompt-rendering block.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PromptSpec {
    /// `MiniJinja` template body. When `None`, the canonical skeleton is
    /// used at render time.
    pub template: Option<String>,
}

/// A full template as authored on disk.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Template {
    #[serde(default = "current_schema_version")]
    pub schema_version: u32,
    pub id: TemplateId,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Monotonic version counter, bumped on content-changing save.
    #[serde(default = "Template::default_version")]
    pub version: u32,
    #[serde(default)]
    pub tags: Vec<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    #[serde(default)]
    pub preamble: Preamble,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub derived: BTreeMap<String, DerivedSpec>,
    #[serde(default, rename = "fields")]
    pub fields: Vec<Field>,
    #[serde(default, rename = "groups", skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<FieldGroup>,
    #[serde(default)]
    pub prompt: PromptSpec,
}

impl Template {
    pub const fn default_version() -> u32 {
        1
    }

    pub fn new(name: impl Into<String>, now: Timestamp) -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            id: TemplateId::new(),
            name: name.into(),
            description: String::new(),
            version: 1,
            tags: Vec::new(),
            created_at: now,
            updated_at: now,
            preamble: Preamble::default(),
            derived: BTreeMap::new(),
            fields: Vec::new(),
            groups: Vec::new(),
            prompt: PromptSpec::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fields::{Field, FieldKind, FieldOptions, Validation};

    fn sample_template() -> Template {
        let now = Timestamp::parse("2026-04-24T10:00:00Z").unwrap();
        let mut t = Template::new("refactor-module", now);
        t.preamble.markdown = "Rust codebase.".into();
        t.fields.push(Field {
            id: "module_path".into(),
            kind: FieldKind::FilePicker,
            label: "Target module".into(),
            help: None,
            placeholder: None,
            required: true,
            default: None,
            show_if: None,
            validation: Validation {
                regex: Some("^src/.*\\.rs$".into()),
                ..Validation::default()
            },
            options: FieldOptions::default(),
        });
        t
    }

    #[test]
    fn template_roundtrips_through_toml() {
        let t = sample_template();
        let s = toml::to_string(&t).unwrap();
        let back: Template = toml::from_str(&s).unwrap();
        assert_eq!(t, back);
    }
}
