//! Built-in templates seeded on first run.
//!
//! Seeding is intentionally conservative: we only write defaults when the
//! user's templates directory is empty.

use lattice_core::fields::{Field, FieldKind, FieldOptions, Validation};
use lattice_core::time::Timestamp;

use lattice_core::entities::{PromptSpec, Template};

pub(crate) fn default_templates(now: Timestamp) -> Vec<Template> {
    let mut t = Template::new("Code Builder", now);
    t.description = "Default template seeded on first run.".into();
    t.fields = vec![
        Field {
            id: "description".into(),
            kind: FieldKind::Textarea,
            label: "What to do".into(),
            help: None,
            placeholder: Some(
                "Describe the change you want, constraints, and acceptance criteria.".into(),
            ),
            required: true,
            default: None,
            show_if: None,
            validation: Validation::default(),
            options: FieldOptions { options: vec![] },
        },
        Field {
            id: "diagrams".into(),
            kind: FieldKind::SequenceGram,
            label: "Sequence diagrams".into(),
            help: Some("Add one or more diagrams (F3 opens the editor).".into()),
            placeholder: None,
            required: false,
            default: None,
            show_if: None,
            validation: Validation::default(),
            options: FieldOptions { options: vec![] },
        },
        Field {
            id: "code_blocks".into(),
            kind: FieldKind::CodeBlocks,
            label: "Code blocks".into(),
            help: Some("Add one or more named code blocks (F4 opens the editor).".into()),
            placeholder: None,
            required: false,
            default: None,
            show_if: None,
            validation: Validation::default(),
            options: FieldOptions { options: vec![] },
        },
        Field {
            id: "test_cases".into(),
            kind: FieldKind::Gherkin,
            label: "Test cases (Gherkin)".into(),
            help: Some("Add Gherkin scenarios (F5 opens the editor).".into()),
            placeholder: None,
            required: false,
            default: None,
            show_if: None,
            validation: Validation::default(),
            options: FieldOptions { options: vec![] },
        },
        Field {
            id: "api_contract".into(),
            kind: FieldKind::OpenApi,
            label: "API Contract (OpenAPI)".into(),
            help: Some("Define endpoints with select/cycle inputs (F6 opens the editor).".into()),
            placeholder: None,
            required: false,
            default: None,
            show_if: None,
            validation: Validation::default(),
            options: FieldOptions { options: vec![] },
        },
    ];
    t.prompt = PromptSpec {
        template: r#"# Role
You are an autonomous senior engineer working on this repository.
- Prefer small, correct diffs over large rewrites.
- Keep changes aligned with existing patterns, naming, and formatting.
- If there is ambiguity, pick the most reasonable approach and document the assumption in the final delivery.

# Goal
{{ task.fields.description }}

{% if task.fields.diagrams is defined and task.fields.diagrams %}
# Sequence diagrams (source of truth)
{{ task.fields.diagrams }}
{% endif %}

{% if task.fields.code_blocks is defined and task.fields.code_blocks %}
# Code blocks
{{ task.fields.code_blocks }}
{% endif %}

{% if task.fields.test_cases is defined and task.fields.test_cases %}
# Test cases (Gherkin)
{{ task.fields.test_cases | gherkin_block }}
{% endif %}

{% if task.fields.api_contract is defined and task.fields.api_contract %}
# API contract (OpenAPI)
{{ task.fields.api_contract | code_block("yaml") }}
{% endif %}

# Working mode (autonomous)
- Do not ask the user for feedback or ask intermediate questions; execute end-to-end.
- Only ask a question if it is **strictly necessary** (blocking) and cannot be safely inferred.
- Delegate internally: break the goal into steps and carry them out without confirmation.
- Keep changes minimal but complete; prioritize reviewable diffs.
- Follow repository conventions and avoid introducing technical debt.
- If you make assumptions, list them explicitly in the final delivery.

# Delivery
- Summary of changes and rationale.
- Concrete test plan (commands to run).
- Risks / follow-ups if applicable."#
            .to_string(),
    };
    vec![t]
}
