//! Desktop notifications, best effort: `osascript` on macOS, `notify-send` elsewhere.
//! Titles always start with the tab name so a program can't impersonate Glasshouse (anti-spoofing).

use std::process::{Command, Stdio};

use shell_integration::CommandRecord;

use crate::session::ApprovalContext;

/// (title, body) for a long command that finished in a tab the user isn't looking at.
pub fn command_finished_text(tab: &str, record: &CommandRecord) -> (String, String) {
    let status = match record.exit_code {
        Some(0) => "✓".to_owned(),
        code => format!("✗ exit {}", code.unwrap_or(-1)),
    };
    let command = record.command_line.as_deref().unwrap_or("command");
    let seconds = record.duration().map_or(0.0, |d| d.as_secs_f64());
    (
        tab.to_owned(),
        format!("{status} {command} · {seconds:.0}s"),
    )
}

/// (title, body) for a sudo prompt waiting in a hidden tab.
pub fn approval_waiting_text(tab: &str, ctx: &ApprovalContext) -> (String, String) {
    (
        format!("{tab}: waiting for Touch ID"),
        format!(
            "sudo {} (as {}) · source: {}",
            ctx.command,
            ctx.run_as,
            ctx.source.as_str()
        ),
    )
}

/// Posts a notification without blocking; failures (no notifier installed) are ignored.
pub fn post(title: &str, body: &str) {
    let mut command = if cfg!(target_os = "macos") {
        let script = format!(
            "display notification {} with title {}",
            applescript_string(body),
            applescript_string(title)
        );
        let mut c = Command::new("/usr/bin/osascript");
        c.args(["-e", &script]);
        c
    } else {
        let mut c = Command::new("notify-send");
        c.args(["--app-name=Glasshouse", "--", title, body]);
        c
    };
    let spawned = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    if let Ok(mut child) = spawned {
        // Reap off the UI thread so the child doesn't linger as a zombie.
        std::thread::spawn(move || child.wait());
    }
}

/// A double-quoted AppleScript literal: program-controlled text can't break out of it.
fn applescript_string(s: &str) -> String {
    let escaped: String = s
        .chars()
        .filter(|c| !c.is_control())
        .flat_map(|c| match c {
            '"' | '\\' => vec!['\\', c],
            _ => vec![c],
        })
        .collect();
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use shell_integration::{CommandTimeline, ShellEvent};

    use super::*;
    use crate::session::{ApprovalPhase, ApprovalSource};

    fn finished(exit: i32, seconds: u64) -> CommandRecord {
        let mut timeline = CommandTimeline::new();
        let start = SystemTime::UNIX_EPOCH;
        timeline.apply_at(&ShellEvent::PromptStart, 0, start);
        timeline.apply_at(&ShellEvent::InputStart, 0, start);
        timeline.set_command_line("make test");
        timeline.apply_at(&ShellEvent::OutputStart, 1, start);
        timeline
            .apply_at(
                &ShellEvent::CommandFinished {
                    exit_code: Some(exit),
                },
                5,
                start + Duration::from_secs(seconds),
            )
            .expect("a finished record")
    }

    #[test]
    fn command_finished_text_should_show_a_tick_for_success() {
        assert_eq!(
            command_finished_text("build", &finished(0, 12)),
            ("build".to_owned(), "✓ make test · 12s".to_owned())
        );
    }

    #[test]
    fn command_finished_text_should_show_the_exit_status_for_failure() {
        assert_eq!(
            command_finished_text("build", &finished(2, 7)).1,
            "✗ exit 2 make test · 7s"
        );
    }

    #[test]
    fn approval_waiting_text_should_name_tab_command_and_source() {
        let ctx = ApprovalContext {
            command: "rm -rf /tmp/x".into(),
            run_as: "root".into(),
            host: "mac".into(),
            tab_title: "ops".into(),
            directory: None,
            source: ApprovalSource::Kernel,
            requester_pid: None,
            phase: ApprovalPhase::WaitingForTouchId,
        };
        assert_eq!(
            approval_waiting_text("ops", &ctx),
            (
                "ops: waiting for Touch ID".to_owned(),
                "sudo rm -rf /tmp/x (as root) · source: kernel process table".to_owned()
            )
        );
    }

    #[test]
    fn applescript_string_should_escape_quotes_and_backslashes() {
        assert_eq!(applescript_string(r#"a"b\c"#), r#""a\"b\\c""#);
    }
}
