//! Custom `MiniJinja` filters used in prompt templates.
//!
//! See `docs/TEMPLATES.md §6.3` for the full list and semantics.

// MiniJinja's filter trait works cleanly with owned `Value` / `Kwargs`
// arguments; rewriting every filter to take references just to satisfy
// `needless_pass_by_value` would hide that convention. Filters that
// can't fail are wrapped in `Result` to keep the signature uniform
// across the module.
#![allow(
    clippy::needless_pass_by_value,
    clippy::unnecessary_wraps,
    clippy::unused_self
)]

use minijinja::value::Kwargs;
use minijinja::{Environment, Error, ErrorKind, Value};

pub fn register_filters(env: &mut Environment<'_>) {
    env.add_filter("bullet", bullet);
    env.add_filter("indent", indent);
    env.add_filter("code_block", code_block);
    env.add_filter("gherkin_block", gherkin_block);
    env.add_filter("quote", quote);
    env.add_filter("truncate", truncate);
    env.add_filter("sequence_gram", sequence_gram);
}

/// Render an iterable/string as a Markdown bullet list.
///
/// - Arrays → one `- item` per element.
/// - Strings → split on newlines, each non-empty line becomes a bullet.
/// - Anything else → a single bullet with the value's string form.
fn bullet(v: Value) -> Result<Value, Error> {
    let out = if let Some(s) = v.as_str() {
        s.lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| format!("- {}", l.trim_start_matches(['-', '*', ' '])))
            .collect::<Vec<_>>()
            .join("\n")
    } else if let Ok(iter) = v.try_iter() {
        iter.map(|item| format!("- {}", value_to_display(&item)))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        format!("- {}", value_to_display(&v))
    };
    Ok(Value::from(out))
}

/// Prefix each line of a string with `n` spaces.
fn indent(v: Value, n: i64) -> Result<Value, Error> {
    let n = usize::try_from(n)
        .map_err(|_| Error::new(ErrorKind::InvalidOperation, "indent: negative width"))?;
    let s = value_to_string(&v);
    let pad = " ".repeat(n);
    let out = s
        .lines()
        .map(|l| format!("{pad}{l}"))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(Value::from(out))
}

/// Wrap a string in a fenced code block.
///
/// Usage: `{{ value | code_block }}` or `{{ value | code_block(lang="rust") }}`.
fn code_block(v: Value, kwargs: Kwargs) -> Result<Value, Error> {
    let lang: String = kwargs.get::<Option<String>>("lang")?.unwrap_or_default();
    kwargs.assert_all_used()?;
    let body = value_to_string(&v);
    Ok(Value::from(format!("```{lang}\n{body}\n```")))
}

/// Wrap Gherkin text in a fenced `gherkin` code block.
///
/// Usage: `{{ value | gherkin_block }}`.
fn gherkin_block(v: Value) -> Result<Value, Error> {
    let body = value_to_string(&v);
    Ok(Value::from(format!("```gherkin\n{body}\n```")))
}

/// Prefix each line with `> ` (Markdown quote).
fn quote(v: Value) -> Result<Value, Error> {
    let s = value_to_string(&v);
    let out = s
        .lines()
        .map(|l| format!("> {l}"))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(Value::from(out))
}

/// Cap the string to `n` bytes, adding an ellipsis marker when truncated.
fn truncate(v: Value, n: i64) -> Result<Value, Error> {
    let n = usize::try_from(n)
        .map_err(|_| Error::new(ErrorKind::InvalidOperation, "truncate: negative length"))?;
    let s = value_to_string(&v);
    if s.len() <= n {
        return Ok(Value::from(s));
    }
    // Be careful not to split a multi-byte char.
    let mut cut = n.min(s.len());
    while !s.is_char_boundary(cut) && cut > 0 {
        cut -= 1;
    }
    Ok(Value::from(format!("{}...", &s[..cut])))
}

/// Render a tuigram-like sequence diagram as Mermaid.
///
/// Accepts either:
/// - a full Mermaid body starting with `sequenceDiagram`, or
/// - the lines *inside* a Mermaid `sequenceDiagram` block (we'll add the header).
///
/// Output is always fenced as a Mermaid code block.
fn sequence_gram(v: Value) -> Result<Value, Error> {
    let raw = value_to_string(&v);
    // If the user provided already-fenced Mermaid (or multiple diagrams rendered
    // by the sequence editor), pass it through unchanged. This avoids nesting
    // ```mermaid fences when templates use `| sequence_gram` on a field that
    // already contains fenced blocks.
    //
    // Heuristic: the editor output always includes ```mermaid fences.
    if raw.contains("```mermaid") {
        // The sequence editor stores already-fenced Mermaid (possibly multiple diagrams).
        // Keep the Mermaid blocks intact, but prepend a single legend explaining the
        // out-of-band `edgeContext` section and relation ids.
        let legend = concat!(
            "[Rx] refer to the 1-based relation line number in the Mermaid diagram(s) below.\n",
            "edgeContext refer to extra per-relation notes keyed by relation id.\n\n"
        );
        // Only add the legend if the stored content actually includes edgeContext or
        // relation ids. Otherwise keep output identical to the input.
        let needs_legend = raw.contains("edgeContext:") || raw.contains("[R");
        let reordered = if raw.contains("edgeContext:") {
            move_edge_context_after_mermaid(&raw)
        } else {
            raw.clone()
        };
        if !needs_legend {
            return Ok(Value::from(reordered));
        }
        if reordered.contains("[Rx] refer to") || reordered.starts_with(legend) {
            return Ok(Value::from(reordered));
        }
        return Ok(Value::from(format!("{legend}{reordered}")));
    }
    let mut lines: Vec<&str> = raw.lines().collect();

    // Trim leading/trailing blank lines without allocating a new string.
    while matches!(lines.first(), Some(l) if l.trim().is_empty()) {
        lines.remove(0);
    }
    while matches!(lines.last(), Some(l) if l.trim().is_empty()) {
        lines.pop();
    }

    let has_header = lines
        .first()
        .is_some_and(|l| l.trim_start().starts_with("sequenceDiagram"));

    let mut body = String::new();
    if !has_header {
        body.push_str("sequenceDiagram\n");
    }
    if !lines.is_empty() {
        body.push_str(&lines.join("\n"));
        body.push('\n');
    }

    Ok(Value::from(format!("```mermaid\n{body}```")))
}

fn move_edge_context_after_mermaid(src: &str) -> String {
    // Our stored format (from the TUI) is typically:
    //   ## Name
    //   edgeContext:
    //   [R1]: ...
    //   ```mermaid
    //   ...
    //   ```
    //
    // For prompt output, we want Mermaid blocks first (UI-friendly), then edgeContext.
    let mut out: Vec<String> = Vec::new();
    let mut pending_edge: Vec<String> = Vec::new();
    let mut in_mermaid = false;

    for line in src.lines() {
        let t = line.trim();

        if !in_mermaid && t == "edgeContext:" {
            pending_edge.clear();
            pending_edge.push("edgeContext:".to_string());
            continue;
        }
        if !in_mermaid && !pending_edge.is_empty() {
            // Keep collecting until we hit a Mermaid fence or a new section.
            if t == "```mermaid" || t.starts_with("## ") || t.starts_with("### ") {
                // Defer emitting; the loop will handle the fence/heading normally.
            } else {
                pending_edge.push(line.to_string());
                continue;
            }
        }

        if t == "```mermaid" {
            in_mermaid = true;
            out.push(line.to_string());
            continue;
        }
        if in_mermaid {
            out.push(line.to_string());
            if t == "```" {
                in_mermaid = false;
                if !pending_edge.is_empty() {
                    out.push(pending_edge.join("\n"));
                    pending_edge.clear();
                }
            }
            continue;
        }

        // If we hit a new heading while having pending edgeContext, flush it
        // before the new section to avoid it "leaking" across diagrams.
        if (t.starts_with("## ") || t.starts_with("### ")) && !pending_edge.is_empty() {
            out.push(pending_edge.join("\n"));
            pending_edge.clear();
        }
        out.push(line.to_string());
    }

    if !pending_edge.is_empty() {
        out.push(pending_edge.join("\n"));
    }

    out.join("\n")
}

fn value_to_string(v: &Value) -> String {
    v.as_str().map_or_else(|| v.to_string(), ToOwned::to_owned)
}

fn value_to_display(v: &Value) -> String {
    v.as_str().map_or_else(|| v.to_string(), ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use minijinja::Environment;

    fn env() -> Environment<'static> {
        let mut e = Environment::new();
        register_filters(&mut e);
        e
    }

    fn render(src: &str, ctx: impl serde::Serialize) -> String {
        let e = env();
        let t = e.template_from_str(src).unwrap();
        t.render(minijinja::Value::from_serialize(&ctx)).unwrap()
    }

    #[test]
    fn bullet_from_array() {
        let out = render(
            "{{ items | bullet }}",
            serde_json::json!({ "items": ["a", "b", "c"] }),
        );
        assert_eq!(out, "- a\n- b\n- c");
    }

    #[test]
    fn bullet_from_multiline_string() {
        let out = render(
            "{{ s | bullet }}",
            serde_json::json!({ "s": "first line\nsecond line\n" }),
        );
        assert_eq!(out, "- first line\n- second line");
    }

    #[test]
    fn indent_prepends_spaces() {
        let out = render("{{ s | indent(4) }}", serde_json::json!({ "s": "a\nb" }));
        assert_eq!(out, "    a\n    b");
    }

    #[test]
    fn code_block_with_lang() {
        let out = render(
            r#"{{ s | code_block(lang="rust") }}"#,
            serde_json::json!({ "s": "fn main() {}" }),
        );
        assert_eq!(out, "```rust\nfn main() {}\n```");
    }

    #[test]
    fn code_block_default_lang_is_empty() {
        let out = render(r"{{ s | code_block }}", serde_json::json!({ "s": "plain" }));
        assert_eq!(out, "```\nplain\n```");
    }

    #[test]
    fn gherkin_block_wraps_as_gherkin_fence() {
        let out = render(
            r"{{ s | gherkin_block }}",
            serde_json::json!({ "s": "Feature: X\n  Scenario: Y\n    Given z" }),
        );
        assert!(out.starts_with("```gherkin\n"));
        assert!(out.contains("Feature: X"));
        assert!(out.ends_with("\n```") || out.ends_with("```"));
    }

    #[test]
    fn quote_prefixes_each_line() {
        let out = render("{{ s | quote }}", serde_json::json!({ "s": "one\ntwo" }));
        assert_eq!(out, "> one\n> two");
    }

    #[test]
    fn truncate_caps_bytes() {
        let out = render(
            "{{ s | truncate(4) }}",
            serde_json::json!({ "s": "abcdefgh" }),
        );
        assert_eq!(out, "abcd...");
    }

    #[test]
    fn truncate_respects_char_boundary() {
        let out = render(
            "{{ s | truncate(3) }}",
            serde_json::json!({ "s": "a\u{1F600}bc" }),
        );
        // '\u{1F600}' is 4 bytes; 3 cuts back to 1.
        assert!(out.ends_with("..."));
    }

    #[test]
    fn truncate_noop_when_short() {
        let out = render(
            "{{ s | truncate(100) }}",
            serde_json::json!({ "s": "short" }),
        );
        assert_eq!(out, "short");
    }

    #[test]
    fn sequence_gram_wraps_mermaid_and_adds_header_when_missing() {
        let out = render(
            "{{ s | sequence_gram }}",
            serde_json::json!({ "s": "participant A\nA->>B: Hi" }),
        );
        assert!(out.starts_with("```mermaid\nsequenceDiagram\n"));
        assert!(out.contains("participant A"));
        assert!(out.ends_with("```\n") || out.ends_with("```"));
    }

    #[test]
    fn sequence_gram_preserves_existing_header() {
        let out = render(
            "{{ s | sequence_gram }}",
            serde_json::json!({ "s": "sequenceDiagram\n  A->>B: Hi" }),
        );
        assert!(out.starts_with("```mermaid\nsequenceDiagram\n  A->>B: Hi\n```"));
    }

    #[test]
    fn sequence_gram_passes_through_pre_fenced_mermaid() {
        let src = "## Diagram\n```mermaid\nsequenceDiagram\n  participant A\n```\n";
        let out = render("{{ s | sequence_gram }}", serde_json::json!({ "s": src }));
        assert_eq!(out, src);
    }

    #[test]
    fn sequence_gram_moves_edge_context_after_mermaid_when_pre_fenced() {
        let src =
            "## Diagram\nedgeContext:\n[R1]: note\n```mermaid\nsequenceDiagram\n  A->>B: Hi\n```\n";
        let out = render("{{ s | sequence_gram }}", serde_json::json!({ "s": src }));
        let mermaid_pos = out.find("```mermaid").unwrap();
        let edge_pos = out.find("edgeContext:").unwrap();
        assert!(
            edge_pos > mermaid_pos,
            "edgeContext should come after Mermaid"
        );
        assert!(out.contains("[R1]: note") || out.contains("[R1] -> note"));
    }

    #[test]
    fn sequence_gram_preserves_mermaid_notes() {
        let src = "## Diagram\n```mermaid\nsequenceDiagram\n  participant John\n  John->>John: Hi<br/>there\n  Note over John: Context<br/>two lines\n```\n";
        let out = render("{{ s | sequence_gram }}", serde_json::json!({ "s": src }));
        assert_eq!(out, src);
        assert!(out.contains("Note over John: Context<br/>two lines"));
    }
}
