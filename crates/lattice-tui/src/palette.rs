//! Command palette — the `Ctrl+K` / `/` overlay.
//!
//! We match a small static registry of commands by fuzzy substring.
//! The shell only needs [`candidates`] (for drawing the list) and
//! [`resolve`] (for dispatching on Enter).

use crate::model::{Msg, Screen};

#[derive(Clone, Copy, Debug)]
pub struct Command {
    pub id: &'static str,
    pub label: &'static str,
    pub hint: &'static str,
    pub msg: fn() -> Msg,
}

pub const COMMANDS: &[Command] = &[
    Command {
        id: "go-templates",
        label: "Go: Templates",
        hint: "open the Templates screen",
        msg: || Msg::GoScreen(Screen::Templates),
    },
    Command {
        id: "go-tasks",
        label: "Go: Tasks",
        hint: "open the Tasks screen",
        msg: || Msg::GoScreen(Screen::Tasks),
    },
    Command {
        id: "go-info",
        label: "Go: Info",
        hint: "app info",
        msg: || Msg::GoScreen(Screen::Info),
    },
    Command {
        id: "help",
        label: "Help",
        hint: "show keybindings",
        msg: || Msg::ShowHelp,
    },
    Command {
        id: "new-template",
        label: "New Template",
        hint: "create a template",
        msg: || Msg::OpenCreateTemplate,
    },
    Command {
        id: "new-task",
        label: "New Task",
        hint: "create a task in the selected project",
        msg: || Msg::OpenCreateTask,
    },
    Command {
        id: "quit",
        label: "Quit",
        hint: "exit lattice",
        msg: || Msg::Quit,
    },
];

/// Candidates matching `query` (substring, case-insensitive) in the
/// order they appear in `COMMANDS`.
pub fn candidates(query: &str) -> Vec<&'static Command> {
    if query.is_empty() {
        return COMMANDS.iter().collect();
    }
    let q = query.to_ascii_lowercase();
    COMMANDS
        .iter()
        .filter(|c| c.label.to_ascii_lowercase().contains(&q) || c.id.contains(&q))
        .collect()
}

/// Resolve the currently-highlighted candidate to a `Msg`.
pub fn resolve(query: &str, cursor: usize) -> Option<Msg> {
    let list = candidates(query);
    if list.is_empty() {
        return None;
    }
    let idx = cursor.min(list.len() - 1);
    Some((list[idx].msg)())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_query_returns_all() {
        assert_eq!(candidates("").len(), COMMANDS.len());
    }

    #[test]
    fn filter_by_substring_case_insensitive() {
        let r = candidates("QUIT");
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].id, "quit");
    }

    #[test]
    fn resolve_uses_cursor() {
        let msg = resolve("go", 0);
        assert!(matches!(msg, Some(Msg::GoScreen(Screen::Templates))));
    }
}
