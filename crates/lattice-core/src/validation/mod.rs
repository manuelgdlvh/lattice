//! Task-level validation: a filled task against its template.
//!
//! A field is validated only if it is **visible** — i.e., its `show_if`
//! predicate evaluates to true (or the field has no predicate). Hidden
//! fields are skipped entirely, even if `required = true`. This matches
//! the `docs/TEMPLATES.md` contract.

use minijinja::{Environment, Value as JValue};

use crate::entities::{Task, Template};
use crate::error::{CoreError, FieldError, FieldErrorKind, ValidationError};
use crate::fields::{Field, validate_field};

/// Validate a task against its template.
pub fn validate_task(template: &Template, task: &Task) -> Result<(), CoreError> {
    let mut errors: Vec<FieldError> = Vec::new();
    let ctx = build_context(template, task);

    for field in &template.fields {
        if !field_visible(field, &ctx)? {
            continue;
        }
        let value = task.fields.get(&field.id);
        errors.extend(validate_field(field, value));
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(CoreError::Validation(ValidationError::new(errors)))
    }
}

fn field_visible(field: &Field, ctx: &JValue) -> Result<bool, CoreError> {
    let Some(expr) = field.show_if.as_deref() else {
        return Ok(true);
    };
    let env = Environment::new();
    match env.compile_expression(expr) {
        Ok(compiled) => match compiled.eval(ctx.clone()) {
            Ok(v) => Ok(v.is_true()),
            Err(e) => Err(CoreError::Validation(ValidationError::single(
                FieldError::new(
                    &field.id,
                    FieldErrorKind::Custom(format!("show_if evaluation failed: {e}")),
                ),
            ))),
        },
        Err(e) => Err(CoreError::Validation(ValidationError::single(
            FieldError::new(
                &field.id,
                FieldErrorKind::Custom(format!("invalid show_if expression: {e}")),
            ),
        ))),
    }
}

fn build_context(template: &Template, task: &Task) -> JValue {
    JValue::from_serialize(serde_json::json!({
        "task": { "fields": task.fields },
        "template": { "id": template.id.to_string(), "name": template.name, "version": template.version },
        "derived": task.derived,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fields::{Field, FieldKind, FieldOptions, OptionItem, Validation};
    use crate::ids::TemplateId;
    use crate::time::Timestamp;
    use serde_json::json;

    fn now() -> Timestamp {
        Timestamp::parse("2026-04-24T10:00:00Z").unwrap()
    }

    fn base_template() -> Template {
        let mut t = Template::new("t", now());
        t.fields = vec![
            Field {
                id: "goals".into(),
                kind: FieldKind::Multiselect,
                label: "Goals".into(),
                help: None,
                placeholder: None,
                required: true,
                default: None,
                show_if: None,
                validation: Validation::default(),
                options: FieldOptions {
                    options: vec![
                        OptionItem::Bare("performance".into()),
                        OptionItem::Bare("readability".into()),
                    ],
                    ..FieldOptions::default()
                },
            },
            Field {
                id: "perf_budget_ms".into(),
                kind: FieldKind::Textarea,
                label: "Perf budget".into(),
                help: None,
                placeholder: None,
                required: true,
                default: None,
                show_if: Some("'performance' in task.fields.goals".into()),
                validation: Validation {
                    ..Validation::default()
                },
                options: FieldOptions::default(),
            },
        ];
        t
    }

    fn task_for(template: &Template) -> Task {
        Task::new(TemplateId::new(), template.version, "t", now())
    }

    #[test]
    fn required_field_missing_raises() {
        let t = base_template();
        let task = task_for(&t);
        let err = validate_task(&t, &task).unwrap_err();
        let CoreError::Validation(v) = err else {
            unreachable!()
        };
        // goals required; perf budget hidden (goals missing -> show_if false).
        assert_eq!(v.errors.len(), 1);
        assert_eq!(v.errors[0].field_id, "goals");
    }

    #[test]
    fn show_if_hides_field_even_when_required() {
        let t = base_template();
        let mut task = task_for(&t);
        task.fields.insert("goals".into(), json!(["readability"]));
        // perf_budget_ms is hidden because 'performance' not selected.
        validate_task(&t, &task).unwrap();
    }

    #[test]
    fn show_if_surfaces_required_when_visible() {
        let t = base_template();
        let mut task = task_for(&t);
        task.fields.insert("goals".into(), json!(["performance"]));
        // perf_budget_ms now visible and required but missing.
        let err = validate_task(&t, &task).unwrap_err();
        let CoreError::Validation(v) = err else {
            unreachable!()
        };
        assert!(v.errors.iter().any(|e| e.field_id == "perf_budget_ms"));
    }

    #[test]
    fn invalid_show_if_expression_is_reported() {
        let mut t = base_template();
        t.fields[1].show_if = Some("this is (( not valid".into());
        let mut task = task_for(&t);
        task.fields.insert("goals".into(), json!(["performance"]));
        let err = validate_task(&t, &task).unwrap_err();
        let CoreError::Validation(v) = err else {
            unreachable!()
        };
        assert!(v.errors.iter().any(|e| e.field_id == "perf_budget_ms"));
    }
}
