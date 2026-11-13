use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use crate::{FileUrl, ShellEvent};

/// Process-unique identifier of a [`CommandRecord`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CommandId(u64);

impl CommandId {
    fn next() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

/// Outcome of a [`CommandRecord`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandStatus {
    /// Started (C) but not finished (D).
    Running,
    /// Exited with status 0.
    Succeeded,
    /// Exited with a non-zero status.
    Failed,
    /// Never ran, or finished without reporting a status.
    Unknown,
}

/// One command the shell ran, reconstructed from OSC 133 marks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandRecord {
    /// Stable identity for UI lists.
    pub id: CommandId,
    /// Absolute scrollback row where the prompt began (A). Used for jump-to-prompt and gutter marks.
    pub prompt_row: usize,
    /// Row where output began (C), if the command ran.
    pub output_row: Option<usize>,
    /// Row of the D mark — end of output.
    pub end_row: Option<usize>,
    /// The typed command, trimmed.
    pub command_line: Option<String>,
    /// Working directory (OSC 7) when the prompt appeared.
    pub working_directory: Option<FileUrl>,
    /// When output began (C).
    pub started_at: Option<SystemTime>,
    /// When the command finished (D).
    pub finished_at: Option<SystemTime>,
    /// Exit status from D.
    pub exit_code: Option<i32>,
}

impl CommandRecord {
    fn new(prompt_row: usize, working_directory: Option<FileUrl>) -> Self {
        Self {
            id: CommandId::next(),
            prompt_row,
            output_row: None,
            end_row: None,
            command_line: None,
            working_directory,
            started_at: None,
            finished_at: None,
            exit_code: None,
        }
    }

    /// Started and not yet finished.
    pub fn is_running(&self) -> bool {
        self.started_at.is_some() && self.finished_at.is_none()
    }

    /// Run time; measured up to now while still running. `None` if the command never started.
    pub fn duration(&self) -> Option<Duration> {
        self.duration_at(SystemTime::now())
    }

    /// Like [`duration`](Self::duration), with `now` used for still-running commands.
    pub fn duration_at(&self, now: SystemTime) -> Option<Duration> {
        let start = self.started_at?;
        // A clock that went backwards reads as zero rather than failing.
        Some(
            self.finished_at
                .unwrap_or(now)
                .duration_since(start)
                .unwrap_or_default(),
        )
    }

    /// Running, succeeded, failed or unknown.
    pub fn status(&self) -> CommandStatus {
        if self.is_running() {
            return CommandStatus::Running;
        }
        match self.exit_code {
            None => CommandStatus::Unknown,
            Some(0) => CommandStatus::Succeeded,
            Some(_) => CommandStatus::Failed,
        }
    }
}

/// Folds shell events into a list of commands for the scrollbar gutter, sticky header,
/// finish notifications and "copy output". The engine supplies the cursor's absolute row.
#[derive(Debug, Clone)]
pub struct CommandTimeline {
    commands: Vec<CommandRecord>,
    cwd: Option<FileUrl>,
    /// Commands longer than this trigger a "finished" notification when the window isn't key.
    pub notify_after: Duration,
}

impl Default for CommandTimeline {
    fn default() -> Self {
        Self {
            commands: Vec::new(),
            cwd: None,
            notify_after: Duration::from_secs(5),
        }
    }
}

impl CommandTimeline {
    /// An empty timeline.
    pub fn new() -> Self {
        Self::default()
    }

    /// All commands, oldest first.
    pub fn commands(&self) -> &[CommandRecord] {
        &self.commands
    }

    /// The last reported working directory.
    pub fn cwd(&self) -> Option<&FileUrl> {
        self.cwd.as_ref()
    }

    /// The most recent command.
    pub fn current(&self) -> Option<&CommandRecord> {
        self.commands.last()
    }

    /// Applies `event` at the current time. See [`apply_at`](Self::apply_at).
    pub fn apply(&mut self, event: &ShellEvent, cursor_row: usize) -> Option<CommandRecord> {
        self.apply_at(event, cursor_row, SystemTime::now())
    }

    /// Applies `event`, observed with the cursor on absolute row `cursor_row`.
    ///
    /// Returns a record when a command just finished, so the UI can notify.
    pub fn apply_at(
        &mut self,
        event: &ShellEvent,
        cursor_row: usize,
        now: SystemTime,
    ) -> Option<CommandRecord> {
        match event {
            ShellEvent::PromptStart => {
                self.commands
                    .push(CommandRecord::new(cursor_row, self.cwd.clone()));
            }
            ShellEvent::OutputStart => {
                let last = self.commands.last_mut()?;
                last.output_row = Some(cursor_row);
                last.started_at = Some(now);
            }
            ShellEvent::CommandFinished { exit_code } => {
                let last = self.commands.last_mut()?;
                // A finish without a start is an empty Return at the prompt.
                last.started_at?;
                last.end_row = Some(cursor_row);
                last.finished_at = Some(now);
                last.exit_code = *exit_code;
                return Some(last.clone());
            }
            ShellEvent::WorkingDirectory(url) => self.cwd = Some(url.clone()),
            ShellEvent::InputStart
            | ShellEvent::Notification { .. }
            | ShellEvent::Progress { .. } => {}
        }
        None
    }

    /// The engine reads the typed command between B and C; the app records it here.
    pub fn set_command_line(&mut self, text: &str) {
        if let Some(last) = self.commands.last_mut() {
            last.command_line = Some(text.trim().to_owned());
        }
    }

    /// The last command whose prompt is above `row`.
    pub fn previous_prompt(&self, row: usize) -> Option<&CommandRecord> {
        self.commands.iter().rev().find(|c| c.prompt_row < row)
    }

    /// The first command whose prompt is below `row`.
    pub fn next_prompt(&self, row: usize) -> Option<&CommandRecord> {
        self.commands.iter().find(|c| c.prompt_row > row)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeline_builds_records() {
        let mut t = CommandTimeline::new();
        let t0 = SystemTime::UNIX_EPOCH;
        t.apply_at(&ShellEvent::PromptStart, 10, t0);
        t.set_command_line("make test ");
        t.apply_at(&ShellEvent::OutputStart, 11, t0);
        let done = t
            .apply_at(
                &ShellEvent::CommandFinished { exit_code: Some(1) },
                40,
                t0 + Duration::from_secs(7),
            )
            .unwrap();
        assert_eq!(done.command_line.as_deref(), Some("make test"));
        assert_eq!(done.status(), CommandStatus::Failed);
        assert_eq!(done.duration(), Some(Duration::from_secs(7)));
        assert_eq!((done.output_row, done.end_row), (Some(11), Some(40)));
        assert_eq!(t.previous_prompt(20).map(|c| c.prompt_row), Some(10));
    }

    #[test]
    fn finish_without_start_is_ignored() {
        let mut t = CommandTimeline::new();
        t.apply(&ShellEvent::PromptStart, 0);
        // Empty Return at the prompt.
        assert_eq!(
            t.apply(&ShellEvent::CommandFinished { exit_code: Some(0) }, 1),
            None
        );
    }

    #[test]
    fn events_before_any_prompt_are_ignored() {
        let mut t = CommandTimeline::new();
        assert_eq!(t.apply(&ShellEvent::OutputStart, 3), None);
        t.set_command_line("ls");
        assert!(t.commands().is_empty());
    }

    #[test]
    fn prompt_records_working_directory_and_running_status() {
        let mut t = CommandTimeline::new();
        let dir = FileUrl {
            host: "mac".into(),
            path: "/tmp".into(),
        };
        t.apply(&ShellEvent::WorkingDirectory(dir.clone()), 0);
        t.apply(&ShellEvent::PromptStart, 2);
        assert_eq!(t.current().unwrap().status(), CommandStatus::Unknown);
        t.apply(&ShellEvent::OutputStart, 3);
        let current = t.current().unwrap();
        assert_eq!(current.working_directory.as_ref(), Some(&dir));
        assert_eq!(current.status(), CommandStatus::Running);
        assert_eq!(t.cwd(), Some(&dir));
        assert_eq!(t.notify_after, Duration::from_secs(5));
    }

    #[test]
    fn navigates_between_prompts() {
        let mut t = CommandTimeline::new();
        for row in [0, 10, 20] {
            t.apply(&ShellEvent::PromptStart, row);
        }
        assert_eq!(t.previous_prompt(10).map(|c| c.prompt_row), Some(0));
        assert_eq!(t.previous_prompt(0), None);
        assert_eq!(t.next_prompt(10).map(|c| c.prompt_row), Some(20));
        assert_eq!(t.next_prompt(20), None);
    }

    #[test]
    fn succeeded_status_for_zero_exit() {
        let mut t = CommandTimeline::new();
        t.apply(&ShellEvent::PromptStart, 0);
        t.apply(&ShellEvent::OutputStart, 1);
        let done = t
            .apply(&ShellEvent::CommandFinished { exit_code: Some(0) }, 2)
            .unwrap();
        assert_eq!(done.status(), CommandStatus::Succeeded);
    }
}
