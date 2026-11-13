//! Spawns real shells through the engine. Skipped when `/bin/sh` is missing.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use shell_integration::{LaunchPlan, ShellEvent};
use terminal_core::{EngineEvent, GridSize, PtyTerminal, PtyTerminalOptions, TerminalEngine};

fn sh_plan(script: &str) -> Option<LaunchPlan> {
    if !Path::new("/bin/sh").exists() {
        eprintln!("skipping: /bin/sh not found");
        return None;
    }
    let environment: HashMap<String, String> =
        [("PATH", "/usr/bin:/bin"), ("TERM", "xterm-256color")]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect();
    Some(LaunchPlan {
        executable: "/bin/sh".into(),
        args: vec!["-c".into(), script.into()],
        exec_name: "sh".into(),
        environment,
    })
}

/// Collects events until the shell exits.
fn events_until_exit(rx: &Receiver<EngineEvent>) -> Vec<EngineEvent> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut events = Vec::new();
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(left) {
            Ok(event) => {
                let exited = matches!(event, EngineEvent::Exited { .. });
                events.push(event);
                if exited {
                    return events;
                }
            }
            Err(_) => break,
        }
    }
    panic!("shell did not exit; events so far: {events:?}");
}

#[test]
fn output_and_osc133_marks_reach_snapshot_and_events() {
    let Some(plan) = sh_plan(
        r"printf '\033]133;A\007hello\n\033]133;C\033\\world\n\033]133;D;3\007\033]0;my title\007'; exit 4",
    ) else {
        return;
    };
    let (mut engine, rx) = PtyTerminal::new(PtyTerminalOptions::default());
    let wakes = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&wakes);
    engine.set_waker(Some(Arc::new(move || {
        counter.fetch_add(1, Ordering::Relaxed);
    })));
    engine.start(&plan).unwrap();
    assert!(engine.shell_pid().is_some());
    assert!(engine.pty_master_fd().is_some());

    let events = events_until_exit(&rx);
    let shell: Vec<(ShellEvent, usize)> = events
        .iter()
        .filter_map(|e| match e {
            EngineEvent::Shell { event, row } => Some((event.clone(), *row)),
            _ => None,
        })
        .collect();
    assert_eq!(
        shell,
        [
            (ShellEvent::PromptStart, 0),
            (ShellEvent::OutputStart, 1),
            (ShellEvent::CommandFinished { exit_code: Some(3) }, 2)
        ]
    );
    assert!(events.contains(&EngineEvent::Title("my title".into())));
    assert!(events.contains(&EngineEvent::OutputActivity));
    assert_eq!(events.last(), Some(&EngineEvent::Exited { code: Some(4) }));
    assert!(wakes.load(Ordering::Relaxed) > 0);

    let snapshot = engine.snapshot();
    assert_eq!((snapshot.columns, snapshot.screen_lines), (80, 24));
    assert_eq!(snapshot.lines[0].text(), "hello");
    assert_eq!(snapshot.lines[1].text(), "world");
    assert_eq!(snapshot.lines[0].spans()[0].text.trim_end(), "hello");
    assert_eq!(engine.text(0, 2), "hello\nworld");
    assert_eq!(engine.cursor_absolute_row(), 2);
    assert_eq!(engine.top_visible_row(), 0);
    assert_eq!(engine.total_rows(), 24);
}

#[test]
fn input_reaches_the_shell_and_colors_resolve() {
    let Some(plan) = sh_plan(r#"read line; printf '\033[1;31m%s\033[0m\n' "got:$line""#) else {
        return;
    };
    let (mut engine, rx) = PtyTerminal::new(PtyTerminalOptions::default());
    engine.start(&plan).unwrap();
    engine.send_text("abc\r").unwrap();
    events_until_exit(&rx);

    let snapshot = engine.snapshot();
    let line = snapshot
        .lines
        .iter()
        .find(|l| l.text().starts_with("got:"))
        .expect("output line");
    assert_eq!(line.text(), "got:abc");
    let cell = &line.cells[0];
    let appearance = terminal_core::TerminalAppearance::default();
    // Bold red is drawn bright with bold-is-bright.
    assert_eq!(cell.fg, appearance.ansi[9]);
    assert!(cell.flags.contains(terminal_core::CellFlags::BOLD));
    assert!(
        engine.send_text("late").is_err(),
        "input after exit is rejected"
    );
}

#[test]
fn resize_emits_event_and_reshapes_grid() {
    let (engine, rx) = PtyTerminal::new(PtyTerminalOptions::default());
    engine.resize(GridSize::new(100, 30)).unwrap();
    assert_eq!(
        rx.try_recv().unwrap(),
        EngineEvent::Resized {
            cols: 100,
            rows: 30
        }
    );
    let snapshot = engine.snapshot();
    assert_eq!((snapshot.columns, snapshot.lines.len()), (100, 30));
}

#[test]
fn missing_shell_is_reported() {
    let plan = LaunchPlan {
        executable: "/nonexistent/shell".into(),
        args: Vec::new(),
        exec_name: "-shell".into(),
        environment: HashMap::new(),
    };
    let (mut engine, _rx) = PtyTerminal::new(PtyTerminalOptions::default());
    assert!(matches!(
        engine.start(&plan),
        Err(terminal_core::EngineError::ShellNotExecutable(_))
    ));
}

#[test]
fn login_shell_plan_runs_and_reports_exit_code() {
    let Some(mut plan) = sh_plan("") else { return };
    plan.args.clear();
    plan.exec_name = "-sh".into();
    let (mut engine, rx) = PtyTerminal::new(PtyTerminalOptions::default());
    engine.start(&plan).unwrap();
    engine.send_text("exit 7\r").unwrap();
    assert_eq!(
        events_until_exit(&rx).last(),
        Some(&EngineEvent::Exited { code: Some(7) })
    );
}

#[cfg(unix)]
#[test]
fn detects_secret_input_from_pty_line_discipline() {
    use terminal_core::{PasswordPromptDetector, PasswordPromptState};

    let Some(plan) = sh_plan("stty -echo; printf 'Password:'; read secret") else {
        return;
    };
    let (mut engine, rx) = PtyTerminal::new(PtyTerminalOptions::default());
    engine.start(&plan).unwrap();
    let fd = engine.pty_master_fd().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while PasswordPromptDetector::state(fd) != PasswordPromptState::SecretInput {
        assert!(
            Instant::now() < deadline,
            "termios never showed ECHO off + ICANON on"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    engine.send_text("hunter2\r").unwrap();
    events_until_exit(&rx);
}
