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
mod skeleton;

pub use filters::register_filters;
pub use skeleton::canonical_template;

use minijinja::{Environment, UndefinedBehavior, Value as JValue};
use serde::Serialize;

use crate::entities::{Project, Task, Template};
use crate::error::RenderError;
use crate::time::Timestamp;

/// Scope variables the renderer exposes to template authors.
///
/// Mirrors the table in `docs/TEMPLATES.md §6.2`.
#[derive(Debug, Serialize)]
pub struct RenderScope<'a> {
    pub preamble: &'a str,
    pub project: ProjectScope<'a>,
    pub task: TaskScope<'a>,
    pub template: TemplateScope<'a>,
    pub derived: &'a serde_json::Map<String, serde_json::Value>,
    pub now: String,
}

#[derive(Debug, Serialize)]
pub struct ProjectScope<'a> {
    pub id: String,
    pub name: &'a str,
    pub path: String,
    pub description: &'a str,
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

/// The main entry point. Uses the template's custom prompt body if set,
/// falling back to the canonical skeleton otherwise.
pub fn render(
    template: &Template,
    task: &Task,
    project: &Project,
    now: Timestamp,
) -> Result<String, RenderError> {
    let body = template
        .prompt
        .template
        .as_deref()
        .unwrap_or_else(|| canonical_template());

    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    register_filters(&mut env);

    // Convert BTreeMaps to serde_json::Map via a round-trip. The cost
    // is negligible for any realistic template.
    let fields = to_json_map(&task.fields);
    let derived = to_json_map(&task.derived);

    let scope = RenderScope {
        preamble: &template.preamble.markdown,
        project: ProjectScope {
            id: project.id.to_string(),
            name: &project.name,
            path: project.path.to_string_lossy().into_owned(),
            description: &project.description,
        },
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
    use crate::entities::{Preamble, PromptSpec, TaskStatus};
    use serde_json::json;

    fn fixtures() -> (Project, Template, Task) {
        let now = Timestamp::parse("2026-04-24T10:00:00Z").unwrap();
        let project = Project::new("acme", "/tmp/acme", now);
        let mut template = Template::new("bug-fix", now);
        template.preamble = Preamble {
            markdown: "Be careful.".into(),
        };
        template.prompt = PromptSpec {
            template: Some(
                "hello {{ project.name }} / {{ task.fields.ticket }} ({{ template.version }})"
                    .into(),
            ),
        };
        let mut task = Task::new(project.id, template.id, template.version, "t", now);
        task.status = TaskStatus::Draft;
        task.fields.insert("ticket".into(), json!("PROJ-123"));
        (project, template, task)
    }

    #[test]
    fn renders_simple_prompt() {
        let (project, template, task) = fixtures();
        let now = Timestamp::parse("2026-04-24T12:00:00Z").unwrap();
        let out = render(&template, &task, &project, now).unwrap();
        assert_eq!(out, "hello acme / PROJ-123 (1)");
    }

    #[test]
    fn strict_mode_errors_on_undefined() {
        let (project, mut template, task) = fixtures();
        template.prompt = PromptSpec {
            template: Some("{{ task.fields.not_there }}".into()),
        };
        let now = Timestamp::parse("2026-04-24T12:00:00Z").unwrap();
        let err = render(&template, &task, &project, now).unwrap_err();
        assert!(matches!(err, RenderError::Engine(_)));
    }

    #[test]
    fn falls_back_to_canonical_skeleton() {
        let (project, mut template, task) = fixtures();
        template.prompt = PromptSpec::default();
        let now = Timestamp::parse("2026-04-24T12:00:00Z").unwrap();
        let out = render(&template, &task, &project, now).unwrap();
        assert!(out.contains("## Context"));
        assert!(out.contains("acme"));
    }
}
