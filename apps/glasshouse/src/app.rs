//! Window state and everything that changes it: tabs, overlays, settings actions and the frame poll.

use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use glasshouse_security::{SecureKeyboardEntry, TouchIDSudo, TouchIDSudoStatus};
use rune::{KeyEvent, LiveRegion, TextEdit, Tx, Waker};
use shell_integration::CommandRecord;
use ssh_config::{
    ManagedSshConfig, ManagedSshHost, SshConfigConflict, SshConfigStore, SshIncludeStatus,
    SshKeyFile, glasshouse_ssh_scripts,
};
use terminal_core::{GridSize, TerminalAppearance, TerminalEngine};

use crate::appearance::{Accessibility, ThemeLibrary, terminal_appearance};
use crate::hosts::{self, SshHost};
use crate::notify;
use crate::palette::{self, PaletteAction, PaletteHost, PaletteItem, PaletteTab};
use crate::session::{Session, SessionNote};
use crate::shortcuts::{self, Shortcut};

/// Re-read trust this often while sudo or ssh is in the foreground, as the Swift app did.
const TRUST_POLL: Duration = Duration::from_millis(100);
const BLINK: f64 = 0.53;

/// System settings the view reads from `Cx` and the state needs between frames.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SystemEnv {
    pub dark: bool,
    pub increase_contrast: bool,
}

/// One monospace cell, measured from the font at runtime.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CellSize {
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsPane {
    Security,
    Ssh,
    Appearance,
}

pub struct PaletteState {
    pub edit: TextEdit,
    pub selection: usize,
}

/// The fields of the Add Host / Edit Host form.
pub struct HostDraft {
    pub original: Option<String>,
    pub alias: TextEdit,
    pub host_name: TextEdit,
    pub user: TextEdit,
    pub port: TextEdit,
    pub key_name: String,
    pub error: Option<String>,
}

impl HostDraft {
    pub fn new(key_name: String) -> Self {
        HostDraft {
            original: None,
            alias: TextEdit::new(""),
            host_name: TextEdit::new(""),
            user: TextEdit::new(""),
            port: TextEdit::new(""),
            key_name,
            error: None,
        }
    }

    pub fn editing(host: &ManagedSshHost) -> Self {
        HostDraft {
            original: Some(host.alias.clone()),
            alias: TextEdit::new(&host.alias),
            host_name: TextEdit::new(&host.host_name),
            user: TextEdit::new(host.user.as_deref().unwrap_or_default()),
            port: TextEdit::new(host.port.map(|p| p.to_string()).unwrap_or_default()),
            key_name: host.key_name.clone(),
            error: None,
        }
    }

    /// The host this draft describes; a non-numeric port is reported as out of range, as in Swift.
    pub fn host(&self) -> ManagedSshHost {
        let port_text = self.port.text.trim();
        let port = (!port_text.is_empty()).then(|| port_text.parse::<i64>().unwrap_or(-1));
        ManagedSshHost::new(
            &self.alias.text,
            &self.host_name.text,
            Some(&self.user.text),
            port,
            &self.key_name,
        )
    }
}

pub struct SshSettings {
    pub store: Option<SshConfigStore>,
    pub keys: Vec<SshKeyFile>,
    pub managed: ManagedSshConfig,
    pub include: SshIncludeStatus,
    pub conflicts: Vec<SshConfigConflict>,
    pub new_key: TextEdit,
    pub editor: Option<HostDraft>,
    pub error: Option<String>,
}

impl SshSettings {
    fn load() -> Self {
        let mut ssh = SshSettings {
            store: SshConfigStore::standard().ok(),
            keys: Vec::new(),
            managed: ManagedSshConfig::new(),
            include: SshIncludeStatus::Active,
            conflicts: Vec::new(),
            new_key: TextEdit::new("glasshouse"),
            editor: None,
            error: None,
        };
        ssh.reload();
        ssh
    }

    pub fn reload(&mut self) {
        let Some(store) = &self.store else {
            self.error = Some("No home directory: set $HOME so Glasshouse can find ~/.ssh".into());
            return;
        };
        self.keys = store.secure_enclave_keys();
        self.include = store.include_status();
        match store.load() {
            Ok(config) => {
                self.conflicts = store.conflicts(&config);
                self.managed = config;
                self.error = None;
            }
            Err(e) => self.error = Some(format!("Couldn't read the Glasshouse host list: {e}")),
        }
        let taken: Vec<&str> = self.keys.iter().map(|k| k.name.as_str()).collect();
        if taken.contains(&self.new_key.text.as_str()) {
            self.new_key = TextEdit::new(free_key_name(&taken));
        }
    }

    pub fn can_create_key(&self) -> bool {
        let name = self.new_key.text.as_str();
        ManagedSshHost::is_valid_key_name(name) && !self.keys.iter().any(|k| k.name == name)
    }
}

/// "glasshouse", then "glasshouse-2", "glasshouse-3", …
pub fn free_key_name(taken: &[&str]) -> String {
    if !taken.contains(&"glasshouse") {
        return "glasshouse".into();
    }
    (2..)
        .map(|n| format!("glasshouse-{n}"))
        .find(|name| !taken.contains(&name.as_str()))
        .unwrap_or_default()
}

pub struct SettingsState {
    pub pane: SettingsPane,
    pub touch_id: TouchIDSudoStatus,
    pub include_reattach: bool,
    pub ssh: SshSettings,
    pub appearance_error: Option<String>,
}

impl SettingsState {
    fn open(pane: SettingsPane) -> Self {
        SettingsState {
            pane,
            touch_id: TouchIDSudo::status(),
            include_reattach: false,
            ssh: SshSettings::load(),
            appearance_error: None,
        }
    }
}

pub struct App {
    pub sessions: Vec<Session>,
    pub selected: Option<u64>,
    next_id: u64,
    waker: Option<Waker>,
    pub themes: ThemeLibrary,
    pub env: SystemEnv,
    pub cell: Option<CellSize>,
    /// The terminal area (inside the gutter), from layout.
    pub pane_size: (f32, f32),
    pub show_sidebar: bool,
    pub palette: Option<PaletteState>,
    pub find: Option<TextEdit>,
    pub settings: Option<SettingsState>,
    pub hosts: Vec<SshHost>,
    hosts_rx: Option<Receiver<Vec<SshHost>>>,
    pub hovered_tab: Option<u64>,
    pub hovered_tick: Option<usize>,
    pub blink_on: bool,
    pub secure_always: bool,
    /// The window's size in points, from layout (the wallpaper places its light by it).
    pub window: (f32, f32),
    /// Where an edit lands if its field's overlay closed between the event and the handler.
    pub scratch: TextEdit,
    pub scratch_draft: Option<HostDraft>,
    /// The system appearance the last view saw; `poll` applies it (the view can't mutate state).
    pub seen_env: std::cell::Cell<Option<SystemEnv>>,
    /// Started by `setup`: until then (headless snapshots) no shell is spawned.
    started: bool,
    had_tabs: bool,
}

impl Default for App {
    fn default() -> Self {
        App::new(ThemeLibrary::load())
    }
}

impl App {
    pub fn new(themes: ThemeLibrary) -> Self {
        App {
            sessions: Vec::new(),
            selected: None,
            next_id: 1,
            waker: None,
            themes,
            env: SystemEnv {
                dark: true,
                increase_contrast: false,
            },
            cell: None,
            pane_size: (0.0, 0.0),
            show_sidebar: false,
            palette: None,
            find: None,
            settings: None,
            hosts: Vec::new(),
            hosts_rx: None,
            hovered_tab: None,
            hovered_tick: None,
            blink_on: true,
            secure_always: false,
            window: (0.0, 0.0),
            scratch: TextEdit::new(""),
            scratch_draft: None,
            seen_env: std::cell::Cell::new(None),
            started: false,
            had_tabs: false,
        }
    }

    /// A live window (shells spawn), not a snapshot scene or the demo.
    pub fn is_live(&self) -> bool {
        self.started
    }

    /// Case-insensitive matches of the find query on the visible screen.
    pub fn find_matches(&self) -> usize {
        let (Some(query), Some(session)) = (self.find.as_ref(), self.selected()) else {
            return 0;
        };
        let q = query.text.to_lowercase();
        if q.is_empty() {
            return 0;
        }
        session
            .engine()
            .snapshot()
            .lines
            .iter()
            .map(|l| l.text().to_lowercase().matches(&q).count())
            .sum()
    }

    pub fn selected(&self) -> Option<&Session> {
        self.sessions.iter().find(|s| Some(s.id) == self.selected)
    }

    fn selected_index(&self) -> Option<usize> {
        self.sessions
            .iter()
            .position(|s| Some(s.id) == self.selected)
    }

    pub fn current_appearance(&self) -> TerminalAppearance {
        let a11y = Accessibility {
            increase_contrast: self.env.increase_contrast,
            reduce_transparency: false,
        };
        terminal_appearance(&self.themes.resolved(self.env.dark), a11y)
    }

    /// Called once by Rune with the waker that engine threads use to request a frame.
    pub fn setup(&mut self, waker: Waker) {
        self.waker = Some(waker);
        self.started = true;
        self.reload_hosts();
        self.new_tab();
    }

    // MARK: Tabs

    fn next_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn add(&mut self, session: Session) {
        if let Some(size) = self.grid_size() {
            session.resize(size);
        }
        self.selected = Some(session.id);
        self.sessions.push(session);
        self.had_tabs = true;
    }

    /// Adds a tab built by `make` from a fresh id and selects it (demo scenes).
    pub fn add_demo(&mut self, make: impl FnOnce(u64) -> Session) -> u64 {
        let id = self.next_id();
        let session = make(id);
        self.add(session);
        id
    }

    pub fn new_tab(&mut self) {
        let id = self.next_id();
        let session = Session::local(id, self.current_appearance(), self.waker.clone(), true);
        self.add(session);
    }

    pub fn open_host(&mut self, alias: &str) {
        let id = self.next_id();
        let session = Session::ssh(id, alias, self.current_appearance(), self.waker.clone());
        self.add(session);
    }

    /// Runs a script in a new, visible tab (Touch ID setup, Secure Enclave key creation).
    /// The user sees every privileged command — Glasshouse never elevates silently.
    pub fn run_visible_script(&mut self, title: &str, script: &str) -> Result<(), String> {
        let path = std::env::temp_dir().join(format!("glasshouse-{}.sh", uuid::Uuid::new_v4()));
        std::fs::write(&path, script).map_err(|e| {
            format!(
                "Couldn't write the script to {}: {e} — check the temp directory",
                path.display()
            )
        })?;
        self.new_tab();
        let q = format!("'{}'", path.display());
        if let Some(session) = self.sessions.last_mut() {
            session.title = title.to_owned();
            // Show the script, ask, then run it with bash. Works from zsh, bash and fish prompts.
            session.send_text(&format!(
                "clear; cat {q}; echo; printf 'Run these commands? [Return = run, Ctrl-C = cancel] '; \
                 read _ && /bin/bash {q}; rm -f {q}\n"
            ));
        }
        self.settings = None;
        Ok(())
    }

    /// Closing the selected tab selects its right-hand neighbour (else the left), like Safari and Terminal.app.
    pub fn close(&mut self, id: u64) {
        let Some(index) = self.sessions.iter().position(|s| s.id == id) else {
            return;
        };
        let session = self.sessions.remove(index);
        session.close();
        if self.selected == Some(id) {
            self.selected = neighbour_after_close(
                &self.sessions.iter().map(|s| s.id).collect::<Vec<_>>(),
                index,
            );
        }
        self.hovered_tab = None;
        self.update_secure_input();
    }

    pub fn select(&mut self, id: u64) {
        if self.sessions.iter().any(|s| s.id == id) {
            self.selected = Some(id);
            self.hovered_tick = None;
            self.update_secure_input();
        }
    }

    fn select_offset(&mut self, delta: isize) {
        let (Some(i), n) = (self.selected_index(), self.sessions.len()) else {
            return;
        };
        let next = (i as isize + delta).rem_euclid(n as isize) as usize;
        self.selected = Some(self.sessions[next].id);
    }

    // MARK: Layout

    pub fn grid_size(&self) -> Option<GridSize> {
        let cell = self.cell?;
        let a = self
            .selected()
            .map_or_else(|| self.current_appearance(), |s| s.appearance.clone());
        let (w, h) = self.pane_size;
        let cols = ((w - 2.0 * a.padding_horizontal) / cell.width).floor();
        let rows = ((h - 2.0 * a.padding_vertical) / cell.height).floor();
        (cols >= 2.0 && rows >= 1.0).then(|| GridSize::new(cols as u16, rows as u16))
    }

    pub fn set_cell(&mut self, cell: CellSize) {
        if cell.width > 0.0 && cell.height > 0.0 && self.cell != Some(cell) {
            self.cell = Some(cell);
            self.resize_all();
        }
    }

    pub fn set_pane_size(&mut self, w: f32, h: f32) {
        if (w, h) != self.pane_size {
            self.pane_size = (w, h);
            self.resize_all();
        }
    }

    fn resize_all(&self) {
        if let Some(size) = self.grid_size() {
            for session in &self.sessions {
                session.resize(size);
            }
        }
    }

    /// Light/Dark Mode or Increase Contrast changed: every session picks its theme again.
    pub fn set_env(&mut self, env: SystemEnv) {
        if env != self.env {
            self.env = env;
            self.themes.generation += 1;
        }
    }

    // MARK: Frame poll

    pub fn poll(&mut self, clock: f64, tx: &mut Tx) -> bool {
        if let Some(env) = self.seen_env.take() {
            self.set_env(env);
        }
        let mut changed = self.receive_hosts();
        let generation = self.themes.generation;
        if self
            .sessions
            .iter()
            .any(|s| s.appearance_generation != generation)
        {
            let appearance = self.current_appearance();
            for session in &mut self.sessions {
                session.apply_appearance(appearance.clone(), generation);
            }
            self.resize_all();
            changed = true;
        }

        let mut exited = Vec::new();
        let selected = self.selected;
        let mut notes = Vec::new();
        for session in &mut self.sessions {
            let visible = Some(session.id) == selected;
            let (session_changed, session_notes) = session.drain(visible);
            changed |= session_changed;
            notes.extend(session_notes.into_iter().map(|n| (session.id, n)));
        }
        for (id, note) in notes {
            if let SessionNote::Exited(code) = note {
                exited.push((id, code));
            }
            self.handle_note(id, note, tx);
        }
        // Terminal.app default: close the tab when the shell exits cleanly; keep it to show a failure.
        for (id, code) in exited {
            if code == Some(0) {
                self.close(id);
                changed = true;
            }
        }
        if self.started && self.had_tabs && self.sessions.is_empty() {
            tx.close_window();
        }
        if self.sessions.iter().any(|s| s.polling) {
            tx.wake_after(TRUST_POLL);
        }
        changed |= self.update_blink(clock, tx);
        if changed {
            self.update_secure_input();
        }
        changed
    }

    fn handle_note(&mut self, id: u64, note: SessionNote, tx: &mut Tx) {
        let Some(session) = self.sessions.iter().find(|s| s.id == id) else {
            return;
        };
        match note {
            SessionNote::CommandFinished(record) => {
                let long = record
                    .duration()
                    .is_some_and(|d| d >= session.timeline.notify_after);
                if long && Some(id) != self.selected {
                    let (title, body) = notify::command_finished_text(&session.title, &record);
                    notify::post(&title, &body);
                }
            }
            SessionNote::ApprovalAppeared {
                context,
                visible: true,
            } => tx.announce(
                format!(
                    "sudo is waiting for Touch ID: {}, as {}",
                    context.command, context.run_as
                ),
                LiveRegion::Assertive,
            ),
            SessionNote::ApprovalAppeared {
                context,
                visible: false,
            } => {
                let (title, body) = notify::approval_waiting_text(&session.title, &context);
                notify::post(&title, &body);
            }
            SessionNote::Clipboard(text) => tx.copy(text),
            SessionNote::Bell | SessionNote::Exited(_) => {}
        }
    }

    fn update_blink(&mut self, clock: f64, tx: &mut Tx) -> bool {
        let blinking = self.selected().is_some_and(|s| {
            s.appearance.cursor_blink && s.engine().snapshot_rows(0..0).cursor.blinking
        });
        if !blinking {
            let changed = !self.blink_on;
            self.blink_on = true;
            return changed;
        }
        let phase = (clock / BLINK).floor();
        let on = (phase as u64).is_multiple_of(2);
        tx.wake_after(Duration::from_secs_f64(
            ((phase + 1.0) * BLINK - clock).max(0.01),
        ));
        let changed = on != self.blink_on;
        self.blink_on = on;
        changed
    }

    /// Secure Keyboard Entry is on while the visible tab's foreground program reads a secret.
    fn update_secure_input(&self) {
        let focused = self
            .selected()
            .is_some_and(|s| s.trust == design_system::TrustState::SecureInput);
        if let Ok(mut entry) = SecureKeyboardEntry::shared().lock() {
            entry.update(focused);
        }
    }

    pub fn set_secure_always(&mut self, on: bool) {
        self.secure_always = on;
        if let Ok(mut entry) = SecureKeyboardEntry::shared().lock() {
            entry.set_always_on(on);
        }
    }

    // MARK: Hosts

    pub fn reload_hosts(&mut self) {
        let (send, receive) = channel();
        let waker = self.waker.clone();
        let spawned = std::thread::Builder::new()
            .name("ssh-hosts".into())
            .spawn(move || {
                // The receiver may be gone if a newer reload replaced it; nothing to do then.
                let _ = send.send(hosts::load_all());
                if let Some(waker) = waker {
                    waker();
                }
            });
        if spawned.is_ok() {
            self.hosts_rx = Some(receive);
        }
    }

    fn receive_hosts(&mut self) -> bool {
        let Some(hosts) = self.hosts_rx.as_ref().and_then(|rx| rx.try_recv().ok()) else {
            return false;
        };
        self.hosts = hosts;
        self.hosts_rx = None;
        true
    }

    // MARK: Keys

    /// Every key while nothing has focus: app shortcuts first, the rest to the shell.
    pub fn on_key(&mut self, ev: &KeyEvent, tx: &mut Tx) -> bool {
        if let Some(shortcut) = shortcuts::shortcut(ev) {
            self.run_shortcut(shortcut, tx);
            return true;
        }
        if self.palette.is_some() || self.settings.is_some() {
            return false;
        }
        let Some((key, mods)) = shortcuts::terminal_key(ev) else {
            return false;
        };
        match self.selected() {
            Some(session) if !session.exited => {
                session.send_key(&key, mods);
                true
            }
            _ => false,
        }
    }

    pub fn run_shortcut(&mut self, shortcut: Shortcut, tx: &mut Tx) {
        match shortcut {
            Shortcut::NewTab => self.new_tab(),
            Shortcut::CloseTab => {
                if let Some(id) = self.selected {
                    self.close(id);
                }
            }
            Shortcut::CloseWindow => tx.close_window(),
            Shortcut::CommandPalette | Shortcut::OpenHost => self.open_palette(tx),
            Shortcut::PreviousPrompt => self.with_selected(Session::jump_to_previous_prompt),
            Shortcut::NextPrompt => self.with_selected(Session::jump_to_next_prompt),
            Shortcut::CopyLastOutput => {
                if let Some(text) = self.selected().and_then(Session::last_output) {
                    tx.copy(text);
                }
            }
            Shortcut::Clear => self.with_selected(Session::clear),
            Shortcut::Find => {
                self.find = Some(TextEdit::new(""));
                tx.focus("find-field", false);
            }
            Shortcut::ToggleSidebar => self.show_sidebar = !self.show_sidebar,
            Shortcut::Settings => self.open_settings(SettingsPane::Security),
            Shortcut::Copy => {
                // Without a selection model, Copy takes the visible screen's text.
                if let Some(s) = self.selected() {
                    let top = s.engine().top_visible_row();
                    let rows = s.engine().size().rows as usize;
                    tx.copy(terminal_core::TerminalEngine::text(
                        s.engine(),
                        top,
                        top + rows,
                    ));
                }
            }
            Shortcut::Paste => tx.read_clipboard(|app: &mut App, content, _| {
                if let Some(s) = app.selected() {
                    s.paste(content.text());
                }
            }),
            Shortcut::SelectTab(i) => {
                if let Some(id) = self.sessions.get(i).map(|s| s.id) {
                    self.select(id);
                }
            }
            Shortcut::NextTab => self.select_offset(1),
            Shortcut::PreviousTab => self.select_offset(-1),
        }
    }

    fn with_selected(&self, f: impl Fn(&Session)) {
        if let Some(s) = self.selected() {
            f(s);
        }
    }

    // MARK: Palette

    pub fn open_palette(&mut self, tx: &mut Tx) {
        self.palette = Some(PaletteState {
            edit: TextEdit::new(""),
            selection: 0,
        });
        self.reload_hosts();
        tx.focus("palette-field", false);
    }

    pub fn close_palette(&mut self, tx: &mut Tx) {
        self.palette = None;
        tx.blur();
    }

    pub fn palette_results(&self) -> Vec<PaletteItem> {
        let credentials: Vec<&str> = self.hosts.iter().map(|h| h.credential.badge()).collect();
        let hosts: Vec<PaletteHost<'_>> = self
            .hosts
            .iter()
            .zip(&credentials)
            .map(|(h, c)| PaletteHost {
                alias: &h.alias,
                user: h.user.as_deref(),
                host_name: h.host_name.as_deref(),
                credential: c,
            })
            .collect();
        let cwds: Vec<Option<String>> = self
            .sessions
            .iter()
            .map(|s| s.cwd.as_deref().map(crate::ui::home_relative))
            .collect();
        let tabs: Vec<PaletteTab<'_>> = self
            .sessions
            .iter()
            .zip(&cwds)
            .map(|(s, cwd)| PaletteTab {
                id: s.id,
                title: &s.title,
                cwd: cwd.as_deref(),
            })
            .collect();
        let recent = self.recent_commands();
        let recent: Vec<&str> = recent.iter().map(String::as_str).collect();
        let query = self.palette.as_ref().map_or("", |p| p.edit.text.as_str());
        palette::rank(palette::all_items(&hosts, &tabs, &recent), query)
    }

    /// Most recent first, across every tab.
    fn recent_commands(&self) -> Vec<String> {
        let mut records: Vec<&CommandRecord> = self
            .sessions
            .iter()
            .flat_map(|s| s.timeline.commands())
            .filter(|r| r.command_line.is_some())
            .collect();
        records.sort_by_key(|r| std::cmp::Reverse(r.started_at));
        records
            .into_iter()
            .filter_map(|r| r.command_line.as_deref().map(crate::ui::strip_prompt))
            .filter(|c| !c.is_empty())
            .collect()
    }

    pub fn move_palette_selection(&mut self, delta: isize) {
        let count = self.palette_results().len();
        if let Some(p) = &mut self.palette
            && count > 0
        {
            p.selection = (p.selection as isize + delta).clamp(0, count as isize - 1) as usize;
        }
    }

    pub fn run_palette_selection(&mut self, tx: &mut Tx) {
        let index = self.palette.as_ref().map_or(0, |p| p.selection);
        if let Some(item) = self.palette_results().into_iter().nth(index) {
            self.close_palette(tx);
            self.run_palette_action(item.action, tx);
        }
    }

    pub fn run_palette_action(&mut self, action: PaletteAction, tx: &mut Tx) {
        match action {
            PaletteAction::NewTab => self.new_tab(),
            PaletteAction::CopyLastOutput => self.run_shortcut(Shortcut::CopyLastOutput, tx),
            PaletteAction::ToggleSidebar => self.show_sidebar = !self.show_sidebar,
            PaletteAction::SetUpTouchId => self.open_settings(SettingsPane::Security),
            PaletteAction::ManageSshKeys => self.open_settings(SettingsPane::Ssh),
            PaletteAction::OpenHost(alias) => self.open_host(&alias),
            PaletteAction::SelectTab(id) => self.select(id),
            PaletteAction::InsertCommand(command) => {
                if let Some(s) = self.selected() {
                    s.send_text(&command);
                }
            }
        }
    }

    // MARK: Settings

    pub fn open_settings(&mut self, pane: SettingsPane) {
        match &mut self.settings {
            Some(settings) => settings.pane = pane,
            None => self.settings = Some(SettingsState::open(pane)),
        }
    }

    /// Runs a settings script in a visible tab, or shows why it couldn't.
    pub fn request_visible_script(&mut self, title: &str, script: Result<String, String>) {
        let result = script.and_then(|script| self.run_visible_script(title, &script));
        if let (Err(e), Some(settings)) = (result, &mut self.settings) {
            settings.ssh.error = Some(e);
        }
    }

    pub fn create_secure_enclave_key(&mut self) {
        let Some(name) = self.settings.as_ref().map(|s| s.ssh.new_key.text.clone()) else {
            return;
        };
        let script =
            glasshouse_ssh_scripts::create_secure_enclave_key(&name).map_err(|e| e.to_string());
        self.request_visible_script("SSH key setup", script);
    }

    pub fn install_keys(&mut self, alias: &str) {
        let Some(settings) = &self.settings else {
            return;
        };
        let ssh = &settings.ssh;
        let script = (|| {
            let store = ssh.store.as_ref().ok_or("No home directory")?;
            let host = ssh
                .managed
                .hosts()
                .iter()
                .find(|h| h.alias == alias)
                .ok_or("Host not found")?;
            let key = store
                .public_key(&host.key_name)
                .map_err(|e| e.to_string())?;
            let security = match &host.security_key {
                Some(sk) => Some(store.public_key(&sk.key_name).map_err(|e| e.to_string())?),
                None => None,
            };
            glasshouse_ssh_scripts::install_keys(host, &key, security.as_ref())
                .map_err(|e| e.to_string())
        })();
        self.request_visible_script(&format!("Install keys on {alias}"), script);
    }

    pub fn install_include(&mut self) {
        let Some(ssh) = self.settings.as_mut().map(|s| &mut s.ssh) else {
            return;
        };
        if let Some(store) = &ssh.store
            && let Err(e) = store.install_include()
        {
            ssh.error = Some(format!("~/.ssh/config: {e}"));
            return;
        }
        ssh.reload();
    }

    pub fn save_host_draft(&mut self) {
        let Some(ssh) = self.settings.as_mut().map(|s| &mut s.ssh) else {
            return;
        };
        let Some(draft) = &mut ssh.editor else { return };
        let Some(store) = &ssh.store else { return };
        let host = draft.host();
        let result = store
            .load()
            .map_err(|e| e.to_string())
            .and_then(|mut config| {
                config
                    .upsert(&host, draft.original.as_deref())
                    .map_err(|e| e.to_string())?;
                store.save(&config).map_err(|e| e.to_string())
            });
        match result {
            Ok(()) => {
                ssh.editor = None;
                ssh.reload();
                self.reload_hosts();
            }
            Err(e) => draft.error = Some(e),
        }
    }

    pub fn remove_host(&mut self, alias: &str) {
        let Some(ssh) = self.settings.as_mut().map(|s| &mut s.ssh) else {
            return;
        };
        let Some(store) = &ssh.store else { return };
        let result = store
            .load()
            .map_err(|e| e.to_string())
            .and_then(|mut config| {
                config.remove(alias);
                store.save(&config).map_err(|e| e.to_string())
            });
        ssh.reload();
        if let Err(e) = result {
            ssh.error = Some(e);
        }
        self.reload_hosts();
    }

    pub fn update_appearance(
        &mut self,
        change: impl FnOnce(&mut terminal_theme::AppearanceConfig),
    ) {
        let result = self.themes.update_config(change);
        if let Some(settings) = &mut self.settings {
            settings.appearance_error = result.err();
        }
    }
}

/// After removing index `removed`, the tab to select: the right-hand neighbour, else the left one.
pub fn neighbour_after_close(remaining: &[u64], removed: usize) -> Option<u64> {
    if remaining.is_empty() {
        return None;
    }
    remaining.get(removed.min(remaining.len() - 1)).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neighbour_after_close_should_pick_the_right_hand_tab() {
        assert_eq!(neighbour_after_close(&[1, 3, 4], 1), Some(3));
    }

    #[test]
    fn neighbour_after_close_should_pick_the_left_tab_when_closing_the_last() {
        assert_eq!(neighbour_after_close(&[1, 2], 2), Some(2));
    }

    #[test]
    fn neighbour_after_close_should_be_none_when_no_tabs_remain() {
        assert_eq!(neighbour_after_close(&[], 0), None);
    }

    #[test]
    fn free_key_name_should_number_taken_names() {
        assert_eq!(
            free_key_name(&["glasshouse", "glasshouse-2"]),
            "glasshouse-3"
        );
    }

    #[test]
    fn host_draft_should_report_a_non_numeric_port_as_out_of_range() {
        let mut draft = HostDraft::new("k".into());
        draft.port = TextEdit::new("abc");
        assert_eq!(draft.host().port, Some(-1));
    }

    #[test]
    fn app_should_close_the_window_only_after_tabs_existed() {
        let app = App::new(ThemeLibrary::built_in());
        assert!(!app.had_tabs && app.sessions.is_empty());
    }
}
