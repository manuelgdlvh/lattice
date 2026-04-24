//! Canonical prompt skeleton.
//!
//! When a template does not provide its own `[prompt].template`, this
//! skeleton is applied. The shape matches `docs/TEMPLATES.md §7`.
//!
//! It is intentionally forgiving: missing `constraints`, `acceptance`,
//! `deliverables`, `references` fields degrade gracefully to empty
//! bullet lists or a sensible default.

const CANONICAL: &str = r#"## Context
{{ preamble }}

Project: `{{ project.name }}` at `{{ project.path }}`

## Inputs
{% for id, val in task.fields | items -%}
- **{{ id }}**: {{ val }}
{% endfor %}

## Constraints
{%- if task.fields.constraints is defined %}
{% for c in task.fields.constraints %}- {{ c }}
{% endfor %}
{%- else %}
- _none specified_
{%- endif %}

## Acceptance Criteria
{%- if task.fields.acceptance is defined %}
{% for a in task.fields.acceptance %}- {{ a }}
{% endfor %}
{%- else %}
- _none specified_
{%- endif %}

## Deliverables
{{ task.fields.deliverables | default("A minimal, reviewable change.") }}

## References
{%- if task.fields.references is defined %}
{% for r in task.fields.references %}- {{ r }}
{% endfor %}
{%- else %}
- _none_
{%- endif %}
"#;

pub fn canonical_template() -> &'static str {
    CANONICAL
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_has_all_sections() {
        let s = canonical_template();
        for h in [
            "## Context",
            "## Inputs",
            "## Constraints",
            "## Acceptance Criteria",
            "## Deliverables",
            "## References",
        ] {
            assert!(s.contains(h), "missing section {h}");
        }
    }
}
