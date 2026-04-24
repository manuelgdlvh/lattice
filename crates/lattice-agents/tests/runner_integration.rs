//! Integration tests for `AgentRunner` using `lattice-fake-agent`.
//!
//! The fake agent path is injected by Cargo via the magic
//! `CARGO_BIN_EXE_<name>` env var. That means these tests never depend
//! on any real agent being installed, and they run on CI unchanged.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use tempfile::TempDir;
use tokio::time::timeout;

use lattice_agents::{
    AgentManifest, AgentRunner, LogStream, RunHandle, SpawnSpec,
    manifest::{DetectSpec, InvocationMode, InvocationSpec, RuntimeSpec, WorkingDir},
};
use lattice_core::entities::TaskStatus;
use lattice_core::ids::{AgentId, RunId};

fn fake_agent_path() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_lattice-fake-agent"))
}

/// Build a manifest that points at the fake agent with a hard-coded
/// absolute path, so we sidestep the whole PATH-lookup system.
fn manifest(mode: InvocationMode, args: Vec<String>, kill_grace_ms: u64) -> AgentManifest {
    AgentManifest {
        id: AgentId::new("fake"),
        display_name: "Fake".into(),
        binary: fake_agent_path().to_string_lossy().into_owned(),
        detect: DetectSpec::default(),
        invocation: InvocationSpec { mode, args },
        runtime: RuntimeSpec {
            working_dir: WorkingDir::Project,
            kill_grace_ms,
        },
        env: BTreeMap::new(),
    }
}

fn spawn_spec(
    manifest: AgentManifest,
    tmp: &TempDir,
    prompt: &str,
    extra_env: Vec<(String, String)>,
) -> SpawnSpec {
    let run_id = RunId::new();
    let run_dir = tmp.path().join("runs").join(run_id.to_string());
    let project_root = tmp.path().join("project");
    std::fs::create_dir_all(&project_root).unwrap();
    SpawnSpec {
        run_id,
        manifest,
        project_root,
        prompt: prompt.into(),
        stdout_path: run_dir.join("stdout.log"),
        stderr_path: run_dir.join("stderr.log"),
        prompt_file_path: run_dir.join("prompt.txt"),
        extra_env,
    }
}

async fn collect_lines(handle: &RunHandle, stream: LogStream, how_long: Duration) -> Vec<String> {
    let mut rx = handle.subscribe();
    let mut lines = Vec::new();
    let _ = timeout(how_long, async {
        while let Ok(line) = rx.recv().await {
            if line.stream == stream {
                lines.push(line.text);
            }
        }
    })
    .await;
    lines
}

#[tokio::test(flavor = "multi_thread")]
async fn stdin_mode_echoes_prompt_to_stdout_log() {
    let tmp = TempDir::new().unwrap();
    let m = manifest(InvocationMode::Stdin, vec![], 5_000);
    let spec = spawn_spec(
        m,
        &tmp,
        "hello lattice",
        vec![("LATTICE_FAKE_READ_STDIN".into(), "1".into())],
    );
    let stdout_path = spec.stdout_path.clone();

    let handle = AgentRunner::new().spawn(spec).await.unwrap();
    let report = handle.wait().await;

    assert_eq!(report.status, TaskStatus::Succeeded);
    assert_eq!(report.exit_code, Some(0));

    let stdout = std::fs::read_to_string(&stdout_path).unwrap();
    assert!(stdout.contains("hello lattice"), "got: {stdout:?}");
    assert!(stdout.contains("[fake:stdin-done]"), "got: {stdout:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn arg_mode_substitutes_prompt_into_argv() {
    let tmp = TempDir::new().unwrap();
    // Fake echoes argv when ECHO_ARGS=1; the `{prompt}` placeholder is
    // what gets substituted.
    let m = manifest(
        InvocationMode::Arg,
        vec!["--prompt".into(), "{prompt}".into()],
        5_000,
    );
    let spec = spawn_spec(
        m,
        &tmp,
        "arg-prompt-42",
        vec![("LATTICE_FAKE_ECHO_ARGS".into(), "1".into())],
    );
    let stdout_path = spec.stdout_path.clone();

    let handle = AgentRunner::new().spawn(spec).await.unwrap();
    let report = handle.wait().await;
    assert_eq!(report.status, TaskStatus::Succeeded);

    let stdout = std::fs::read_to_string(&stdout_path).unwrap();
    assert!(
        stdout.lines().any(|l| l == "arg-prompt-42"),
        "got: {stdout:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn file_mode_writes_prompt_and_agent_reads_it_back() {
    let tmp = TempDir::new().unwrap();
    let m = manifest(
        InvocationMode::File,
        vec!["--prompt-file".into(), "{prompt_file}".into()],
        5_000,
    );
    let spec = spawn_spec(
        m,
        &tmp,
        "file-prompt-xyz",
        // The fake agent reads a file whose path is given as an env var.
        vec![],
    );
    // The runner writes the prompt to `prompt_file_path`; the manifest
    // passes that path in argv. We tell the fake agent to read the
    // file by env var pointing to the same path.
    let mut spec = spec;
    spec.extra_env.push((
        "LATTICE_FAKE_READ_FILE".into(),
        spec.prompt_file_path.to_string_lossy().into_owned(),
    ));

    let prompt_path = spec.prompt_file_path.clone();
    let stdout_path = spec.stdout_path.clone();

    let handle = AgentRunner::new().spawn(spec).await.unwrap();
    let report = handle.wait().await;
    assert_eq!(report.status, TaskStatus::Succeeded);

    assert!(prompt_path.exists(), "prompt file must be written to disk");
    let on_disk = std::fs::read_to_string(&prompt_path).unwrap();
    assert_eq!(on_disk, "file-prompt-xyz");

    let stdout = std::fs::read_to_string(&stdout_path).unwrap();
    assert!(stdout.contains("file-prompt-xyz"), "got: {stdout:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn non_zero_exit_maps_to_failed_status() {
    let tmp = TempDir::new().unwrap();
    let m = manifest(InvocationMode::Stdin, vec![], 5_000);
    let spec = spawn_spec(
        m,
        &tmp,
        "prompt",
        vec![("LATTICE_FAKE_EXIT_CODE".into(), "7".into())],
    );

    let handle = AgentRunner::new().spawn(spec).await.unwrap();
    let report = handle.wait().await;
    assert_eq!(report.status, TaskStatus::Failed);
    assert_eq!(report.exit_code, Some(7));
}

#[tokio::test(flavor = "multi_thread")]
async fn stderr_is_tee_to_its_own_log() {
    let tmp = TempDir::new().unwrap();
    let m = manifest(InvocationMode::Stdin, vec![], 5_000);
    let spec = spawn_spec(
        m,
        &tmp,
        "prompt",
        vec![("LATTICE_FAKE_STDERR".into(), "a warning happened".into())],
    );
    let stderr_path = spec.stderr_path.clone();

    let handle = AgentRunner::new().spawn(spec).await.unwrap();
    let _ = handle.wait().await;

    let stderr = std::fs::read_to_string(&stderr_path).unwrap();
    assert!(
        stderr.contains("a warning happened"),
        "expected stderr log to contain message, got: {stderr:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn working_directory_is_the_project_root() {
    let tmp = TempDir::new().unwrap();
    let m = manifest(InvocationMode::Stdin, vec![], 5_000);
    let spec = spawn_spec(
        m,
        &tmp,
        "prompt",
        vec![("LATTICE_FAKE_CWD_MARKER".into(), "1".into())],
    );
    let project_root = spec.project_root.clone();
    let stdout_path = spec.stdout_path.clone();

    let handle = AgentRunner::new().spawn(spec).await.unwrap();
    let _ = handle.wait().await;

    let stdout = std::fs::read_to_string(&stdout_path).unwrap();
    let expected = format!("cwd={}", project_root.display());
    assert!(
        stdout.lines().any(|l| {
            // Account for macOS /private/tmp prefix symlink.
            l == expected
                || l.trim_start_matches("cwd=")
                    .ends_with(project_root.file_name().unwrap().to_str().unwrap())
        }),
        "expected a line matching `{expected}` in stdout, got: {stdout:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn kill_terminates_a_long_running_agent() {
    let tmp = TempDir::new().unwrap();
    // Grace window deliberately tiny so the test finishes fast.
    let m = manifest(InvocationMode::Stdin, vec![], 200);
    let spec = spawn_spec(
        m,
        &tmp,
        "prompt",
        vec![("LATTICE_FAKE_SLEEP_MS".into(), "30000".into())],
    );

    let handle = AgentRunner::new().spawn(spec).await.unwrap();

    // Give the child time to actually start.
    tokio::time::sleep(Duration::from_millis(50)).await;

    handle.kill().await;
    let report = timeout(Duration::from_secs(5), handle.wait())
        .await
        .unwrap();
    assert_eq!(report.status, TaskStatus::Killed);
}

#[tokio::test(flavor = "multi_thread")]
async fn live_log_subscribers_see_lines_as_they_are_emitted() {
    let tmp = TempDir::new().unwrap();
    let m = manifest(InvocationMode::Stdin, vec![], 5_000);
    let spec = spawn_spec(
        m,
        &tmp,
        "prompt",
        vec![
            ("LATTICE_FAKE_EMIT_LINES".into(), "5".into()),
            ("LATTICE_FAKE_LINE_DELAY_MS".into(), "5".into()),
        ],
    );

    let handle = AgentRunner::new().spawn(spec).await.unwrap();
    let stdout_lines = collect_lines(&handle, LogStream::Stdout, Duration::from_secs(3)).await;
    let _ = handle.wait().await;

    for i in 0..5 {
        let expected = format!("line {i}");
        assert!(
            stdout_lines.iter().any(|l| l == &expected),
            "missing {expected:?} in {stdout_lines:?}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn wait_is_idempotent() {
    let tmp = TempDir::new().unwrap();
    let m = manifest(InvocationMode::Stdin, vec![], 5_000);
    let spec = spawn_spec(m, &tmp, "prompt", vec![]);

    let handle = AgentRunner::new().spawn(spec).await.unwrap();
    let a = handle.wait().await;
    let b = handle.wait().await;
    assert_eq!(a, b);
}

#[tokio::test(flavor = "multi_thread")]
async fn pid_is_reported_after_spawn() {
    let tmp = TempDir::new().unwrap();
    let m = manifest(InvocationMode::Stdin, vec![], 5_000);
    let spec = spawn_spec(m, &tmp, "prompt", vec![]);

    let handle = AgentRunner::new().spawn(spec).await.unwrap();
    assert!(handle.pid().is_some(), "spawn should record a PID");
    let _ = handle.wait().await;
}
