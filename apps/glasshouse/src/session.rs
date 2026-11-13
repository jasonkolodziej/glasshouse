//! One terminal tab: owns the engine, the OSC 133 timeline and the kernel-derived trust state.

use std::path::Path;
use std::sync::mpsc::Receiver;

use design_system::TrustState;
use glasshouse_security::{ProcessInspector, ProcessSnapshot};
use shell_integration::{CommandRecord, CommandTimeline, LaunchPlan, ShellEvent, ShellIntegrationInstaller};
use terminal_core::{
    EngineEvent, GridSize, Key, Modifiers, PasswordPromptDetector, PasswordPromptState, PtyEnvironment,
    PtyEnvironmentOptions, PtyTerminal, PtyTerminalOptions, TerminalAppearance, TerminalEngine, Waker,
};

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Where the approval card's command came from. Every field the card shows says so ("Source" line).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApprovalSource {
    Kernel,
    Shell,
}

impl ApprovalSource {
    pub fn as_str(self) -> &'static str {
        match self {
            ApprovalSource::Kernel => "kernel process table",
            ApprovalSource::Shell => "reported by shell",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApprovalPhase {
    WaitingForTouchId,
    PasswordFallback,
}

/// What the approval card shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApprovalContext {
    pub command: String,
    pub run_as: String,
    pub host: String,
    pub tab_title: String,
    pub directory: Option<String>,
    pub source: ApprovalSource,
    pub requester_pid: Option<i32>,
    pub phase: ApprovalPhase,
}

/// Something the workspace should react to after draining a session's events.
#[derive(Clone, Debug, PartialEq)]
pub enum SessionNote {
    CommandFinished(CommandRecord),
    /// A sudo prompt appeared; `visible` says whether its tab is on screen (announce) or not (notify).
    ApprovalAppeared { context: ApprovalContext, visible: bool },
    Exited(Option<i32>),
    Bell,
    Clipboard(String),
}

pub struct Session {
    pub id: u64,
    engine: PtyTerminal,
    events: Receiver<EngineEvent>,
    pub title: String,
    pub cwd: Option<String>,
    pub exited: bool,
    pub exit_code: Option<i32>,
    /// Why the shell couldn't start, shown in the pane instead of a grid.
    pub start_error: Option<String>,
    pub timeline: CommandTimeline,
    pub trust: TrustState,
    pub approval: Option<ApprovalContext>,
    /// True while sudo or ssh owns the foreground: trust is re-read every 100 ms.
    pub polling: bool,
    pub appearance: TerminalAppearance,
    pub appearance_generation: u64,
    input_start_row: Option<usize>,
}

impl Session {
    /// A login shell with shell integration (OSC 133) unless `shell_integration` is false.
    pub fn local(id: u64, appearance: TerminalAppearance, waker: Option<Waker>, shell_integration: bool) -> Self {
        let env = PtyEnvironment::make(&PtyEnvironmentOptions::new(APP_VERSION), &PtyEnvironment::current());
        let shell = ShellIntegrationInstaller::user_shell();
        let plan = match ShellIntegrationInstaller::with_default_root() {
            Ok(installer) => installer.plan(&shell, &env, shell_integration),
            Err(_) => LaunchPlan { executable: shell.clone(), args: Vec::new(), exec_name: shell, environment: env },
        };
        Self::with_plan(id, "Shell", &plan, appearance, waker)
    }

    /// `ssh <alias>` via the *system* OpenSSH, which honours SSH_SK_PROVIDER.
    pub fn ssh(id: u64, alias: &str, appearance: TerminalAppearance, waker: Option<Waker>) -> Self {
        let env = PtyEnvironment::make(&PtyEnvironmentOptions::new(APP_VERSION), &PtyEnvironment::current());
        let ssh = if Path::new("/usr/bin/ssh").exists() { "/usr/bin/ssh" } else { "ssh" };
        let plan = LaunchPlan {
            executable: ssh.into(),
            args: vec![alias.to_owned()],
            exec_name: "ssh".into(),
            environment: env,
        };
        Self::with_plan(id, alias, &plan, appearance, waker)
    }

    pub fn with_plan(
        id: u64,
        title: &str,
        plan: &LaunchPlan,
        appearance: TerminalAppearance,
        waker: Option<Waker>,
    ) -> Self {
        let options = PtyTerminalOptions { appearance: appearance.clone(), ..PtyTerminalOptions::default() };
        let (mut engine, events) = PtyTerminal::new(options);
        engine.set_waker(waker);
        let start_error = engine.start(plan).err().map(|e| e.to_string());
        Session {
            id,
            engine,
            events,
            title: title.to_owned(),
            cwd: None,
            exited: start_error.is_some(),
            exit_code: None,
            start_error,
            timeline: CommandTimeline::new(),
            trust: TrustState::Idle,
            approval: None,
            polling: false,
            appearance,
            appearance_generation: 0,
            input_start_row: None,
        }
    }

    pub fn engine(&self) -> &PtyTerminal {
        &self.engine
    }

    pub fn apply_appearance(&mut self, appearance: TerminalAppearance, generation: u64) {
        self.appearance_generation = generation;
        if appearance != self.appearance {
            self.engine.apply(&appearance);
            self.appearance = appearance;
        }
    }

    /// Handles every queued engine event. Returns whether anything changed (the grid needs
    /// redrawing) and what the workspace must act on.
    pub fn drain(&mut self, visible: bool) -> (bool, Vec<SessionNote>) {
        let mut notes = Vec::new();
        let mut activity = false;
        let mut changed = false;
        while let Ok(event) = self.events.try_recv() {
            changed = true;
            match event {
                EngineEvent::Shell { event, row } => self.handle_shell(&event, row, &mut notes),
                EngineEvent::Title(t) => self.title = sanitize_title(&t),
                EngineEvent::OutputActivity | EngineEvent::Resized { .. } => activity = true,
                EngineEvent::Bell => notes.push(SessionNote::Bell),
                EngineEvent::ClipboardStore(text) => notes.push(SessionNote::Clipboard(text)),
                EngineEvent::Exited { code } => {
                    self.exited = true;
                    self.exit_code = code;
                    self.polling = false;
                    self.set_trust(TrustState::Idle, None, visible, &mut notes);
                    notes.push(SessionNote::Exited(code));
                }
            }
        }
        if (activity || self.polling) && !self.exited {
            let before = (self.trust.clone(), self.approval.clone());
            self.refresh_trust(visible, &mut notes);
            changed |= before != (self.trust.clone(), self.approval.clone());
        }
        (changed, notes)
    }

    fn handle_shell(&mut self, event: &ShellEvent, row: usize, notes: &mut Vec<SessionNote>) {
        match event {
            ShellEvent::InputStart => self.input_start_row = Some(row),
            ShellEvent::OutputStart => {
                if let Some(start) = self.input_start_row {
                    // Command text between B and C — display data only, never used for trust.
                    let text = self.engine.text(start, row + 1);
                    self.timeline.set_command_line(&text);
                }
            }
            ShellEvent::WorkingDirectory(url) => self.cwd = Some(url.path.clone()),
            _ => {}
        }
        if let Some(finished) = self.timeline.apply(event, row) {
            notes.push(SessionNote::CommandFinished(finished));
        }
    }

    /// Re-evaluates who owns the foreground and whether the tty is in secret-input mode.
    /// Called on output activity and every 100 ms while sudo/ssh is in the foreground.
    fn refresh_trust(&mut self, visible: bool, notes: &mut Vec<SessionNote>) {
        let Some(fd) = self.engine.pty_master_fd() else { return };
        let foreground = ProcessInspector::foreground(fd);
        let secret = PasswordPromptDetector::state(fd) == PasswordPromptState::SecretInput;
        match foreground {
            Some(fg) if fg.is_ssh() => {
                self.polling = true;
                let dest = fg.ssh_destination().unwrap_or("remote host").to_owned();
                // Local secret mode while ssh is foreground = ssh's own passphrase/password prompt.
                let trust = if secret { TrustState::SecureInput } else { TrustState::RemoteSession(dest) };
                self.set_trust(trust, None, visible, notes);
            }
            Some(fg) if fg.is_sudo() => {
                self.polling = true;
                let phase = if secret { ApprovalPhase::PasswordFallback } else { ApprovalPhase::WaitingForTouchId };
                let ctx = self.approval_context(&fg, phase);
                let trust = if secret { TrustState::SecureInput } else { TrustState::TouchIdSudo };
                self.set_trust(trust, Some(ctx), visible, notes);
            }
            _ if secret => self.set_trust(TrustState::SecureInput, None, visible, notes),
            _ => {
                self.polling = false;
                self.set_trust(TrustState::Idle, None, visible, notes);
            }
        }
    }

    fn approval_context(&self, p: &ProcessSnapshot, phase: ApprovalPhase) -> ApprovalContext {
        // The kernel may not give a setuid-root sudo's argv. Without it, fall back to the
        // shell-reported command line and say so on the card.
        let kernel_args: &[String] = p.arguments.get(1..).unwrap_or_default();
        let (command, source) = if kernel_args.is_empty() {
            let reported = self.timeline.current().and_then(|r| r.command_line.clone());
            (reported.unwrap_or_else(|| "sudo".into()), ApprovalSource::Shell)
        } else {
            let quoted: Vec<String> = kernel_args.iter().map(|a| quote_for_display(a)).collect();
            (quoted.join(" "), ApprovalSource::Kernel)
        };
        ApprovalContext {
            command,
            run_as: sudo_target_user(kernel_args).unwrap_or("root").to_owned(),
            host: host_name(),
            tab_title: self.title.clone(),
            directory: self.cwd.clone(),
            source,
            requester_pid: Some(p.pid),
            phase,
        }
    }

    fn set_trust(
        &mut self,
        trust: TrustState,
        approval: Option<ApprovalContext>,
        visible: bool,
        notes: &mut Vec<SessionNote>,
    ) {
        let appeared = self.approval.is_none() && approval.is_some();
        self.trust = trust;
        self.approval = approval;
        if let Some(ctx) = self.approval.as_ref().filter(|_| appeared) {
            notes.push(SessionNote::ApprovalAppeared { context: ctx.clone(), visible });
        }
    }

    // MARK: Actions

    pub fn send_text(&self, text: &str) {
        // A failed write means the shell has gone; the Exited event reports that.
        let _ = self.engine.send_text(text);
    }

    pub fn send_key(&self, key: &Key, mods: Modifiers) {
        let _ = self.engine.send_key(key, mods);
        self.engine.scroll_to_bottom();
    }

    pub fn paste(&self, text: &str) {
        let _ = self.engine.paste(text);
        self.engine.scroll_to_bottom();
    }

    pub fn resize(&self, size: GridSize) {
        if self.engine.size() != size {
            let _ = self.engine.resize(size);
        }
    }

    pub fn jump_to_previous_prompt(&self) {
        if let Some(r) = self.timeline.previous_prompt(self.engine.top_visible_row()) {
            self.engine.scroll_to_absolute_row(r.prompt_row);
        }
    }

    pub fn jump_to_next_prompt(&self) {
        if let Some(r) = self.timeline.next_prompt(self.engine.top_visible_row()) {
            self.engine.scroll_to_absolute_row(r.prompt_row);
        }
    }

    pub fn jump_to(&self, record: &CommandRecord) {
        self.engine.scroll_to_absolute_row(record.prompt_row);
    }

    pub fn output(&self, record: &CommandRecord) -> String {
        let Some(start) = record.output_row else { return String::new() };
        let end = record.end_row.unwrap_or_else(|| self.engine.cursor_absolute_row());
        self.engine.text(start, end)
    }

    pub fn last_output(&self) -> Option<String> {
        let last = self.timeline.commands().iter().rev().find(|r| r.finished_at.is_some())?;
        Some(self.output(last))
    }

    /// ⌘K: clears the screen (Ctrl-L), as the Swift app did.
    pub fn clear(&self) {
        self.send_text("\u{0C}");
    }

    pub fn close(&self) {
        // An exited shell's PID may already belong to another process.
        if !self.exited {
            self.engine.terminate();
        }
    }
}

/// Program-set titles (OSC 0/2) never reach trusted chrome; strip glyphs that imitate it anyway.
pub fn sanitize_title(raw: &str) -> String {
    const BANNED: [char; 7] = ['🔒', '🔐', '🔑', '🛡', '✅', '☑', '✔'];
    raw.chars().filter(|c| !BANNED.contains(c) && !matches!(c, '\n' | '\r')).take(80).collect()
}

/// The user `sudo -u NAME` / `sudo --user=NAME` runs as.
pub fn sudo_target_user(args: &[String]) -> Option<&str> {
    if let Some(i) = args.iter().position(|a| a == "-u") {
        return args.get(i + 1).map(String::as_str);
    }
    args.iter().find_map(|a| a.strip_prefix("--user="))
}

fn quote_for_display(s: &str) -> String {
    if s.contains(' ') { format!("'{s}'") } else { s.to_owned() }
}

fn host_name() -> String {
    let mut buf = [0u8; 256];
    // SAFETY: `buf` is valid for `buf.len()` bytes; gethostname NUL-terminates on success.
    let ok = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) } == 0;
    let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
    match std::str::from_utf8(&buf[..end]) {
        Ok(name) if ok && !name.is_empty() => name.trim_end_matches(".local").to_owned(),
        _ => "This computer".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn sanitize_title_should_strip_lock_glyphs() {
        assert_eq!(sanitize_title("🔒 bank login ✅"), " bank login ");
    }

    #[test]
    fn sanitize_title_should_drop_newlines() {
        assert_eq!(sanitize_title("a\nb\rc"), "abc");
    }

    #[test]
    fn sanitize_title_should_cap_at_80_characters() {
        assert_eq!(sanitize_title(&"x".repeat(200)).chars().count(), 80);
    }

    #[test]
    fn sudo_target_user_should_read_dash_u() {
        assert_eq!(sudo_target_user(&args(&["-u", "postgres", "psql"])), Some("postgres"));
    }

    #[test]
    fn sudo_target_user_should_read_long_option() {
        assert_eq!(sudo_target_user(&args(&["--user=www", "ls"])), Some("www"));
    }

    #[test]
    fn sudo_target_user_should_be_none_without_an_option() {
        assert_eq!(sudo_target_user(&args(&["ls", "/root"])), None);
    }

    #[test]
    fn quote_for_display_should_quote_arguments_with_spaces() {
        assert_eq!(quote_for_display("a b"), "'a b'");
    }
}
