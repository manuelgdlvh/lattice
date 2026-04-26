//! Prompt rendering.
//!
//! The renderer is a thin wrapper around `MiniJinja` that:
//!
//! - runs in **strict undefined mode** so typos surface as errors,
//! - installs our custom filters (`bullet`, `indent`, `code_block`,
//!   `quote`, `truncate`),
//! - exposes a tight, documented scope (see `docs/TEMPLATES.md §6.2`).
//!
//! Rendering is a pure function of (template, task, project, derived,
//! now). No filesystem access. No time side effects (the caller passes
//! `now`).

mod filters;

pub use filters::register_filters;

use minijinja::{Environment, UndefinedBehavior, Value as JValue};
use serde::Serialize;

use crate::entities::{Task, Template};
use crate::error::RenderError;
use crate::time::Timestamp;

/// Scope variables the renderer exposes to template authors.
///
/// Mirrors the table in `docs/TEMPLATES.md §6.2`.
#[derive(Debug, Serialize)]
pub struct RenderScope<'a> {
    pub task: TaskScope<'a>,
    pub template: TemplateScope<'a>,
    pub derived: &'a serde_json::Map<String, serde_json::Value>,
    pub now: String,
}

#[derive(Debug, Serialize)]
pub struct TaskScope<'a> {
    pub id: String,
    pub name: &'a str,
    pub created_at: String,
    pub fields: &'a serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct TemplateScope<'a> {
    pub id: String,
    pub name: &'a str,
    pub version: u32,
}

/// The main entry point.
pub fn render(template: &Template, task: &Task, now: Timestamp) -> Result<String, RenderError> {
    let body = template.prompt.template.as_str();

    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    register_filters(&mut env);

    // Convert BTreeMaps to serde_json::Map via a round-trip. The cost
    // is negligible for any realistic template.
    let fields = to_json_map(&task.fields);
    let derived = to_json_map(&task.derived);

    let scope = RenderScope {
        task: TaskScope {
            id: task.id.to_string(),
            name: &task.name,
            created_at: task.created_at.to_rfc3339(),
            fields: &fields,
        },
        template: TemplateScope {
            id: template.id.to_string(),
            name: &template.name,
            version: template.version,
        },
        derived: &derived,
        now: now.to_rfc3339(),
    };

    let tmpl = env.template_from_str(body)?;
    let out = tmpl.render(JValue::from_serialize(&scope))?;
    Ok(out)
}

fn to_json_map<S: Serialize>(v: &S) -> serde_json::Map<String, serde_json::Value> {
    match serde_json::to_value(v) {
        Ok(serde_json::Value::Object(m)) => m,
        _ => serde_json::Map::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::PromptSpec;
    use serde_json::json;

    fn fixtures() -> (Template, Task) {
        let now = Timestamp::parse("2026-04-24T10:00:00Z").unwrap();
        let mut template = Template::new("bug-fix", now);
        template.prompt = PromptSpec {
            template: "hello {{ task.fields.ticket }} ({{ template.version }})".into(),
        };
        let mut task = Task::new(template.id, template.version, "t", now);
        task.fields.insert("ticket".into(), json!("PROJ-123"));
        (template, task)
    }

    #[test]
    fn renders_simple_prompt() {
        let (template, task) = fixtures();
        let now = Timestamp::parse("2026-04-24T12:00:00Z").unwrap();
        let out = render(&template, &task, now).unwrap();
        assert_eq!(out, "hello PROJ-123 (1)");
    }

    #[test]
    fn strict_mode_errors_on_undefined() {
        let (mut template, task) = fixtures();
        template.prompt = PromptSpec {
            template: "{{ task.fields.not_there }}".into(),
        };
        let now = Timestamp::parse("2026-04-24T12:00:00Z").unwrap();
        let err = render(&template, &task, now).unwrap_err();
        assert!(matches!(err, RenderError::Engine(_)));
    }
}
