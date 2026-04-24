//! End-to-end render tests using `insta` snapshots.
//!
//! These cover the full path: parse a realistic template TOML, build a
//! task instance with filled field values, render with a fixed `now`,
//! and snapshot the output. Any accidental change to the renderer,
//! filters, skeleton, or field-value formatting surfaces here.

use lattice_core::entities::{Project, Task, TaskStatus, Template};
use lattice_core::prompt::render;
use lattice_core::time::Timestamp;

fn frozen_now() -> Timestamp {
    Timestamp::parse("2026-04-24T12:00:00Z").unwrap()
}

fn project() -> Project {
    let now = Timestamp::parse("2026-04-24T10:00:00Z").unwrap();
    let mut p = Project::new("acme-backend", "/home/manu/code/acme-backend", now);
    p.id = lattice_core::ids::ProjectId::nil();
    p.description = "Payments gateway".into();
    p
}

fn task_for(template: &Template, values: serde_json::Value) -> Task {
    let now = Timestamp::parse("2026-04-24T10:30:00Z").unwrap();
    let mut t = Task::new(
        project().id,
        template.id,
        template.version,
        "bug: fix rate limiter",
        now,
    );
    t.id = lattice_core::ids::TaskId::nil();
    t.status = TaskStatus::Draft;
    if let serde_json::Value::Object(map) = values {
        for (k, v) in map {
            t.fields.insert(k, v);
        }
    }
    t
}

fn load_template(toml_str: &str) -> Template {
    let mut t: Template = toml::from_str(toml_str).expect("template parse");
    // Pin the id + timestamps so snapshots stay stable.
    t.id = lattice_core::ids::TemplateId::nil();
    t.created_at = Timestamp::parse("2026-04-20T09:00:00Z").unwrap();
    t.updated_at = Timestamp::parse("2026-04-20T09:00:00Z").unwrap();
    t
}

#[test]
fn bug_fix_template_renders() {
    let template = load_template(include_str!("fixtures/templates/bug_fix.toml"));
    let task = task_for(
        &template,
        serde_json::json!({
            "ticket": "PAY-1234",
            "symptom": "Rate limiter returns 500 instead of 429 when Redis is slow.",
            "repro_steps": "1. Point REDIS_URL at a slow instance.\n2. Burst 100 req/s for 10s.\n3. Observe 500s.",
            "scope": "surface-fix",
        }),
    );

    let out = render(&template, &task, &project(), frozen_now()).expect("render ok");
    insta::assert_snapshot!("bug_fix_render", out);
}

#[test]
fn canonical_skeleton_renders_without_custom_prompt() {
    // Minimal template with no [prompt].template — falls back to the
    // canonical skeleton. Uses the conventional field names that the
    // skeleton recognizes (`constraints`, `acceptance`, ...).
    let template = load_template(
        r#"
        id = "00000000-0000-0000-0000-000000000000"
        name = "minimal"
        description = ""
        version = 1
        created_at = "2026-04-20T09:00:00Z"
        updated_at = "2026-04-20T09:00:00Z"

        [preamble]
        markdown = "You are a senior Rust engineer."

        [[fields]]
        id = "goal"
        kind = "text"
        label = "Goal"
        required = true

        [[fields]]
        id = "constraints"
        kind = "multiselect"
        label = "Constraints"
        required = false
        options = ["no-new-deps", "preserve-public-api"]

        [[fields]]
        id = "acceptance"
        kind = "multiselect"
        label = "Acceptance"
        required = false
        options = ["tests-green", "clippy-clean"]
        "#,
    );
    let task = task_for(
        &template,
        serde_json::json!({
            "goal": "Refactor auth middleware for testability.",
            "constraints": ["no-new-deps", "preserve-public-api"],
            "acceptance": ["tests-green", "clippy-clean"],
        }),
    );

    let out = render(&template, &task, &project(), frozen_now()).expect("render ok");
    insta::assert_snapshot!("canonical_skeleton_render", out);
}
