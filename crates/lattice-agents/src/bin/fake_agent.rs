//! `lattice-fake-agent`
//!
//! A deterministic, test-only stand-in for a real agent. It has no
//! intelligence at all — it just does exactly what integration tests
//! tell it to do via environment variables, which means tests can run
//! in CI without requiring `cursor-agent` or any other real agent to
//! be installed.
//!
//! ## Environment controls
//!
//! | Var                          | Behavior                                            |
//! |------------------------------|-----------------------------------------------------|
//! | `LATTICE_FAKE_VERSION`       | Print the value to stdout and exit 0 (detection).   |
//! | `LATTICE_FAKE_READ_STDIN=1`  | Read stdin to EOF, re-emit on stdout.               |
//! | `LATTICE_FAKE_ECHO_ARGS=1`   | Print argv[1..] joined by '\n'.                     |
//! | `LATTICE_FAKE_READ_FILE=P`   | Read file `P` and echo to stdout.                   |
//! | `LATTICE_FAKE_EMIT_LINES=N`  | Emit `N` lines of the form `line <i>` with flushes. |
//! | `LATTICE_FAKE_LINE_DELAY_MS` | Per-line sleep used alongside `EMIT_LINES`.         |
//! | `LATTICE_FAKE_SLEEP_MS=N`    | Sleep `N` ms before doing anything else.            |
//! | `LATTICE_FAKE_STDERR=s`      | Write `s\n` to stderr.                              |
//! | `LATTICE_FAKE_EXIT_CODE=N`   | Exit with the given code (default 0).               |
//! | `LATTICE_FAKE_CWD_MARKER=1`  | Print `cwd=<current_dir>` on stdout.                |
//!
//! Behaviors run in the order listed above (sleep happens near the end,
//! right before exit, so callers can test kill-during-sleep paths).

use std::io::{Read, Write};

fn main() {
    let code = match run() {
        Ok(code) => code,
        Err(e) => {
            let _ = writeln!(std::io::stderr(), "lattice-fake-agent error: {e}");
            1
        }
    };
    std::process::exit(code);
}

fn run() -> std::io::Result<i32> {
    if let Ok(v) = std::env::var("LATTICE_FAKE_VERSION") {
        println!("{v}");
        return Ok(0);
    }

    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    if env_flag("LATTICE_FAKE_READ_STDIN") {
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        write!(out, "{buf}")?;
        // Newline marker so tests can assert end-of-echo cleanly even
        // if the prompt did not end in '\n'.
        writeln!(out, "[fake:stdin-done]")?;
        out.flush()?;
    }

    if env_flag("LATTICE_FAKE_ECHO_ARGS") {
        let args: Vec<String> = std::env::args().skip(1).collect();
        for a in args {
            writeln!(out, "{a}")?;
        }
        out.flush()?;
    }

    if let Ok(path) = std::env::var("LATTICE_FAKE_READ_FILE") {
        let body = std::fs::read_to_string(&path)?;
        write!(out, "{body}")?;
        writeln!(out, "[fake:file-done]")?;
        out.flush()?;
    }

    if let Ok(n) = std::env::var("LATTICE_FAKE_EMIT_LINES") {
        let n: u32 = n.parse().unwrap_or(0);
        let delay_ms: u64 = std::env::var("LATTICE_FAKE_LINE_DELAY_MS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        for i in 0..n {
            writeln!(out, "line {i}")?;
            out.flush()?;
            if delay_ms > 0 {
                std::thread::sleep(std::time::Duration::from_millis(delay_ms));
            }
        }
    }

    if env_flag("LATTICE_FAKE_CWD_MARKER") {
        let cwd = std::env::current_dir()?;
        writeln!(out, "cwd={}", cwd.display())?;
        out.flush()?;
    }

    if let Ok(msg) = std::env::var("LATTICE_FAKE_STDERR") {
        let _ = writeln!(std::io::stderr(), "{msg}");
    }

    if let Ok(ms) = std::env::var("LATTICE_FAKE_SLEEP_MS")
        && let Ok(ms) = ms.parse::<u64>()
    {
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }

    let code = std::env::var("LATTICE_FAKE_EXIT_CODE")
        .ok()
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(0);
    Ok(code)
}

fn env_flag(name: &str) -> bool {
    matches!(std::env::var(name).as_deref(), Ok("1" | "true" | "yes"))
}
