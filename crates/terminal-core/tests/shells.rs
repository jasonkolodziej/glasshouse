//! Each supported shell, launched with its integration scripts on a real PTY: the OSC 133 marks
//! arrive in order (A prompt, B input, C output, D finished) and the rows between B and C hold
//! the command. A shell that isn't installed is reported and left out; CI installs all three.

use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use shell_integration::{ShellEvent, ShellIntegrationInstaller};
use terminal_core::{EngineEvent, GridSize, PtyTerminal, PtyTerminalOptions, TerminalEngine};

const DEADLINE: Duration = Duration::from_secs(20);

fn find(shell: &str) -> Option<String> {
    ["/usr/bin", "/bin", "/usr/local/bin", "/opt/homebrew/bin"]
        .iter()
        .map(|dir| format!("{dir}/{shell}"))
        .find(|path| Path::new(path).exists())
}

/// Shell events until `stop` matches one, with the cursor row each arrived at.
fn shell_events_until(
    engine: &PtyTerminal,
    rx: &Receiver<EngineEvent>,
    stop: impl Fn(&ShellEvent) -> bool,
) -> Vec<(ShellEvent, usize)> {
    let deadline = Instant::now() + DEADLINE;
    let mut seen = Vec::new();
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(left) {
            Ok(EngineEvent::Shell { event, row }) => {
                let done = stop(&event);
                seen.push((event, row));
                if done {
                    return seen;
                }
            }
            Ok(EngineEvent::Exited { code }) => panic!("shell exited ({code:?}) after {seen:?}"),
            Ok(_) => {}
            Err(_) => break,
        }
    }
    let screen = engine.text(0, engine.cursor_absolute_row() + 1);
    panic!("timed out; shell events so far: {seen:?}; screen:\n{screen}");
}

fn marks_frame_the_command(shell: &str) {
    let Some(path) = find(shell) else {
        eprintln!("skipping {shell}: not installed");
        return;
    };
    let home = tempfile::tempdir().unwrap_or_else(|e| panic!("temp HOME: {e}"));
    let root = home.path().join("integration");
    let environment: HashMap<String, String> = [
        ("PATH", "/usr/local/bin:/usr/bin:/bin"),
        ("TERM", "xterm-256color"),
        ("HOME", &home.path().to_string_lossy()),
        (
            "XDG_CONFIG_HOME",
            &home.path().join(".config").to_string_lossy(),
        ),
        (
            "XDG_DATA_HOME",
            &home.path().join(".local/share").to_string_lossy(),
        ),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect();
    let plan = ShellIntegrationInstaller::new(root).plan(&path, &environment, true);

    let options = PtyTerminalOptions {
        size: GridSize::new(100, 30),
        ..PtyTerminalOptions::default()
    };
    let (mut engine, rx) = PtyTerminal::new(options);
    engine
        .start(&plan)
        .unwrap_or_else(|e| panic!("{shell} starts: {e}"));

    // The first prompt: A, then B once it is drawn.
    let first = shell_events_until(&engine, &rx, |e| *e == ShellEvent::InputStart);
    assert!(
        first.iter().any(|(e, _)| *e == ShellEvent::PromptStart),
        "{shell}: no A before B: {first:?}"
    );

    // Valid in sh-family shells and fish alike; its output ("glass-house") differs from its text.
    engine
        .send_text("printf '%s-%s\\n' glass house\r")
        .unwrap_or_else(|e| panic!("type a command: {e}"));
    let run = shell_events_until(&engine, &rx, |e| {
        matches!(e, ShellEvent::CommandFinished { .. })
    });
    let row_of = |wanted: &dyn Fn(&ShellEvent) -> bool| {
        first
            .iter()
            .chain(&run)
            .rev()
            .find(|(e, _)| wanted(e))
            .map(|(_, row)| *row)
    };
    let input =
        row_of(&|e| *e == ShellEvent::InputStart).unwrap_or_else(|| panic!("{shell}: no B"));
    let output = row_of(&|e| *e == ShellEvent::OutputStart)
        .unwrap_or_else(|| panic!("{shell}: no C in {run:?}"));
    let finished = run.last().map(|(e, _)| e.clone());
    assert_eq!(
        finished,
        Some(ShellEvent::CommandFinished { exit_code: Some(0) }),
        "{shell}: {run:?}"
    );
    let command = engine.text(input, output.max(input + 1));
    assert!(
        command.contains("printf '%s-%s\\n' glass house"),
        "{shell}: command rows read {command:?}"
    );
    let printed = engine.text(output, engine.cursor_absolute_row() + 1);
    assert!(
        printed.contains("glass-house"),
        "{shell}: output read {printed:?}"
    );

    engine.terminate();
}

#[test]
fn bash_marks_frame_the_command() {
    marks_frame_the_command("bash");
}

#[test]
fn zsh_marks_frame_the_command() {
    marks_frame_the_command("zsh");
}

#[test]
fn fish_marks_frame_the_command() {
    marks_frame_the_command("fish");
}
