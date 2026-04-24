//! Headless rendering smoke tests.
//!
//! Builds a `Model` populated with representative state, then asks
//! ratatui to render into a `TestBackend`. We assert that the screen
//! title appears in the buffer — a very cheap guard against layout
//! regressions (missing tabs, empty status bar) for every screen.

use ratatui::Terminal;
use ratatui::backend::TestBackend;

use lattice_core::entities::{Project, Task, Template};
use lattice_core::ids::{ProjectId, TemplateId};
use lattice_core::time::Timestamp;
use lattice_tui::{Model, Screen};

fn render(model: &Model) -> String {
    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| lattice_tui::view::render(f, model))
        .unwrap();
    let buf = terminal.backend().buffer();
    let mut out = String::new();
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            out.push_str(buf[(x, y)].symbol());
        }
        out.push('\n');
    }
    out
}

fn populated_model() -> Model {
    let now = Timestamp::parse("2026-04-24T10:00:00Z").unwrap();
    let mut m = Model::new();
    let p1 = Project::new("example", "/tmp/example", now);
    let p2 = Project::new("other", "/tmp/other", now);
    let tpl = Template::new("refactor", now);
    let pid = p1.id;
    let tid = tpl.id;
    m.projects = vec![p1, p2];
    m.templates = vec![tpl];
    m.selected_project = Some(pid);
    m.selected_template = Some(tid);
    let t = Task::new(pid, tid, 1, "fix auth bug", now);
    m.tasks_by_project.insert(pid, vec![t]);
    m
}

#[test]
fn every_screen_renders_without_panic() {
    let mut m = populated_model();
    for screen in [
        Screen::Projects,
        Screen::Templates,
        Screen::Tasks,
        Screen::Runtime,
        Screen::History,
        Screen::Info,
        Screen::Help,
    ] {
        m.screen = screen;
        let s = render(&m);
        assert!(s.contains("lattice"), "missing title on {screen:?}");
    }
}

#[test]
fn projects_screen_shows_selected_project() {
    let mut m = populated_model();
    m.screen = Screen::Projects;
    let s = render(&m);
    assert!(
        s.contains("example"),
        "missing project name in render:\n{s}"
    );
}

#[test]
fn palette_overlay_lists_candidates() {
    let mut m = populated_model();
    m.palette_open = true;
    m.palette_input = "new".into();
    let s = render(&m);
    assert!(s.contains("New Project"), "palette missing candidate:\n{s}");
}

#[test]
fn form_overlay_renders_focused_field() {
    use lattice_tui::model::{FormField, FormState, FormSubmit};
    let mut m = populated_model();
    m.form = Some(FormState {
        title: "New project".into(),
        fields: vec![
            FormField::plain("Name", "acme", true, false),
            FormField::plain("Path", "/tmp/acme", true, false),
        ],
        cursor: 0,
        submit: FormSubmit::CreateProject,
    });
    let s = render(&m);
    assert!(s.contains("New project"), "form title missing:\n{s}");
    assert!(s.contains("acme"), "focused value missing:\n{s}");
}

#[test]
fn help_screen_lists_global_keys() {
    let mut m = populated_model();
    m.screen = Screen::Help;
    let s = render(&m);
    assert!(s.contains("Quit"), "help missing Quit entry:\n{s}");
    assert!(
        s.contains("Command palette"),
        "help missing palette line:\n{s}"
    );
}

// Silence warnings about unused helpers in subsets of the tests.
#[allow(dead_code)]
fn _silence(_p: ProjectId, _t: TemplateId) {}
