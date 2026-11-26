//! Window state and everything that changes it: tabs, overlays, settings actions and the frame poll.

use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use glasshouse_security::{SecureKeyboardEntry, TouchIDSudo, TouchIDSudoStatus};
use std::collections::HashMap;

use rune::{Drag, KeyEvent, LiveRegion, Phase, TextEdit, Tx, Waker, WheelEvent};
use shell_integration::CommandRecord;
use ssh_config::{
    ManagedSshConfig, ManagedSshHost, SshConfigConflict, SshConfigStore, SshIncludeStatus,
    SshKeyFile, glasshouse_ssh_scripts,
};
use terminal_core::{GridSize, Modifiers, SelectionKind, TerminalAppearance, TerminalEngine};

use crate::appearance::{Accessibility, ThemeLibrary, terminal_appearance};
use crate::hosts::{self, SshHost};
use crate::notify;
use crate::palette::{self, PaletteAction, PaletteHost, PaletteItem, PaletteTab};
use crate::panes::{self, Axis, DropTarget, Edge, PaneTree};
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

/// How tabs share the window, as the commands name it. The arrangement itself is
/// [`App::split`]; this is what it amounts to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Layout {
    /// One tab at a time.
    #[default]
    Single,
    /// Every tab in a row, left to right.
    SideBySide,
    /// Every tab in a column, top to bottom.
    Stacked,
    /// Arranged by dragging tabs onto panes: any mix of rows and columns, or only some tabs.
    Custom,
}

impl Layout {
    /// The layout after asking for `layout`: asking for the current one goes back to one tab.
    pub fn toggled(self, layout: Layout) -> Layout {
        if self == layout {
            Layout::Single
        } else {
            layout
        }
    }

    /// The rail button's next layout: one tab, side by side, stacked, then one tab again. An
    /// arrangement made by dragging goes back to one tab.
    pub fn next(self) -> Layout {
        match self {
            Layout::Single => Layout::SideBySide,
            Layout::SideBySide => Layout::Stacked,
            Layout::Stacked | Layout::Custom => Layout::Single,
        }
    }
}

/// A tab being dragged out of the strip: where it is and where it would land.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TabDrag {
    pub id: u64,
    /// The pointer, in window points.
    pub point: [f32; 2],
    pub target: Option<DropTarget>,
}

/// Pointer travel before a press on a tab becomes a drag, in points.
const TAB_DRAG_THRESHOLD: f32 = 4.0;

/// A divider being dragged: the split it divides and the weights it started from.
#[derive(Clone, Debug, PartialEq)]
struct DividerDrag {
    path: Vec<usize>,
    index: usize,
    start: (f32, f32),
}

/// Where the pieces a drag aims at are on screen, in window points, from layout.
#[derive(Clone, Debug, Default)]
pub struct DropRects {
    pub strip: [f32; 4],
    pub tabs: HashMap<u64, [f32; 4]>,
    pub panes: HashMap<u64, [f32; 4]>,
    /// Each split's length along its axis, by path from the root, dividers included.
    pub splits: HashMap<Vec<usize>, f32>,
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

/// Where the right-click menu over a pane opened.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerminalMenu {
    pub tab: u64,
    pub x: f32,
    pub y: f32,
}

pub struct App {
    pub sessions: Vec<Session>,
    pub selected: Option<u64>,
    next_id: u64,
    waker: Option<Waker>,
    pub themes: ThemeLibrary,
    pub env: SystemEnv,
    pub cell: Option<CellSize>,
    /// The selected tab's terminal area (inside the gutter), from layout. With one tab on screen
    /// every tab takes this size; split, each takes its own pane's (`Session::pane_size`).
    pub pane_size: (f32, f32),
    /// The split tabs, arranged; `None` for one tab at a time. Always two or more tabs. It is on
    /// screen while the selected tab is one of them; selecting another tab shows that tab alone,
    /// and the split comes back when one of its tabs is selected again.
    pub split: Option<PaneTree>,
    pub tab_drag: Option<TabDrag>,
    divider_drag: Option<DividerDrag>,
    pub rects: DropRects,
    /// The window layer's top-left in window points (inset in the presentation scene).
    pub window_origin: (f32, f32),
    pub show_sidebar: bool,
    pub palette: Option<PaletteState>,
    pub find: Option<TextEdit>,
    pub settings: Option<SettingsState>,
    pub hosts: Vec<SshHost>,
    hosts_rx: Option<Receiver<Vec<SshHost>>>,
    pub hovered_tab: Option<u64>,
    pub hovered_tick: Option<usize>,
    pub blink_on: bool,
    /// Wheel travel not yet worth a whole line, in lines (trackpads send fractions).
    wheel_rest: f32,
    /// The right-click menu over a pane: its tab, and where it opened in window points.
    pub terminal_menu: Option<TerminalMenu>,
    pub secure_always: bool,
    /// The window's size in points, from layout (the wallpaper places its light by it).
    pub window: (f32, f32),
    /// Presentation scenes: the window floats on a wallpaper, with drawn window buttons.
    pub showcase: bool,
    /// A real window (live or `--demo`), not a headless render: below 100% opacity the frame is
    /// the system's Liquid Glass over the desktop. Headless renders have no desktop to show, so
    /// they blur a painted wallpaper instead.
    pub native_glass: bool,
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
            split: None,
            tab_drag: None,
            divider_drag: None,
            rects: DropRects::default(),
            window_origin: (0.0, 0.0),
            show_sidebar: false,
            palette: None,
            find: None,
            settings: None,
            hosts: Vec::new(),
            hosts_rx: None,
            hovered_tab: None,
            hovered_tick: None,
            blink_on: true,
            wheel_rest: 0.0,
            terminal_menu: None,
            secure_always: false,
            window: (0.0, 0.0),
            showcase: false,
            native_glass: false,
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

    /// Whether panes are on screen side by side or stacked: a split layout with 2+ tabs.
    pub fn is_split(&self) -> bool {
        self.split
            .as_ref()
            .zip(self.selected)
            .is_some_and(|(tree, id)| tree.contains(id))
    }

    /// The tabs on screen: the split's when it shows, else the selected one.
    pub fn visible_ids(&self) -> Vec<u64> {
        match &self.split {
            Some(tree) if self.is_split() => tree.leaves(),
            _ => self.selected.into_iter().collect(),
        }
    }

    pub fn session(&self, id: u64) -> Option<&Session> {
        self.sessions.iter().find(|s| s.id == id)
    }

    fn ids(&self) -> Vec<u64> {
        self.sessions.iter().map(|s| s.id).collect()
    }

    /// What the arrangement amounts to: every tab in one row or column is side by side or
    /// stacked; anything else made by dragging is custom.
    pub fn layout(&self) -> Layout {
        let Some(tree) = &self.split else {
            return Layout::Single;
        };
        let every_tab = tree.leaves().len() == self.sessions.len();
        match tree.flat_axis() {
            Some(Axis::Row) if every_tab => Layout::SideBySide,
            Some(Axis::Column) if every_tab => Layout::Stacked,
            _ => Layout::Custom,
        }
    }

    /// Switches to `layout`, or back to one tab if it is already on.
    pub fn toggle_layout(&mut self, layout: Layout) {
        self.set_layout(self.layout().toggled(layout));
    }

    /// Side by side and stacked put every tab in one row or column. A split with one tab open
    /// opens a second one beside it, as splitting a pane does in other terminals, so the layout
    /// change is never invisible.
    pub fn set_layout(&mut self, layout: Layout) {
        let axis = match layout {
            Layout::Single => {
                self.set_split(None);
                return;
            }
            Layout::SideBySide => Axis::Row,
            Layout::Stacked => Axis::Column,
            Layout::Custom => return,
        };
        if self.sessions.len() == 1 && self.started {
            self.new_tab();
        }
        self.set_split(PaneTree::flat(axis, &self.ids()));
    }

    fn set_split(&mut self, split: Option<PaneTree>) {
        // A tree of one tab is no split.
        self.split = split.filter(|t| t.leaves().len() > 1);
        self.hovered_tick = None;
        // Panes report their new sizes on the next layout; until then each tab keeps the size it
        // had, and going back to one tab gives every tab the selected pane's size.
        self.resize_all();
    }

    // MARK: Dragging

    /// A tab in the strip is dragged: past a few points it lifts, and the drop moves it along
    /// the strip or places it on a pane on screen. A press that never travelled stays a click.
    pub fn drag_tab(&mut self, id: u64, d: &Drag) {
        match d.phase {
            Phase::Down => self.tab_drag = None,
            Phase::Move => {
                let lifted = self.tab_drag.is_some() || d.dx.hypot(d.dy) >= TAB_DRAG_THRESHOLD;
                if lifted {
                    let point = d.window_point();
                    self.tab_drag = Some(TabDrag {
                        id,
                        point,
                        target: self.drop_target(point),
                    });
                }
            }
            Phase::Up => {
                if let Some(drag) = self.tab_drag.take()
                    && !d.cancelled()
                    && let Some(target) = self.drop_target(d.window_point())
                {
                    self.drop_tab(drag.id, target);
                }
            }
        }
    }

    fn drop_target(&self, point: [f32; 2]) -> Option<DropTarget> {
        let tabs: Vec<[f32; 4]> = self
            .sessions
            .iter()
            .filter_map(|s| self.rects.tabs.get(&s.id).copied())
            .collect();
        let panes: Vec<(u64, [f32; 4])> = self
            .visible_ids()
            .into_iter()
            .filter_map(|id| self.rects.panes.get(&id).map(|r| (id, *r)))
            .collect();
        let dragged = self.tab_drag.map(|d| d.id);
        // A pane spot where letting go would change nothing is no target, so nothing lights up.
        panes::drop_target(point, self.rects.strip, &tabs, &panes).filter(|t| match (t, dragged) {
            (DropTarget::Pane(pane, edge), Some(id)) => self.arranged(id, *pane, *edge).is_some(),
            _ => true,
        })
    }

    /// The arrangement after dropping tab `id` on `pane`'s `edge`, or `None` if it would change
    /// nothing. Onto a pane of the split, the tab moves there; onto a lone pane, the two become
    /// a split. The lone pane's own tab, dropped on an edge, takes that side and its
    /// neighbour in the strip the other, as dragging the one terminal on screen does in
    /// VS Code.
    pub fn arranged(&self, id: u64, pane: u64, edge: Edge) -> Option<PaneTree> {
        if pane == id {
            if self.is_split() || edge == Edge::Center {
                return None;
            }
            let i = self.sessions.iter().position(|s| s.id == id)?;
            let partner = self
                .sessions
                .get(i + 1)
                .or_else(|| i.checked_sub(1).and_then(|j| self.sessions.get(j)))?
                .id;
            let mut tree = PaneTree::Leaf(partner);
            return tree.place(partner, id, edge).then_some(tree);
        }
        let mut tree = match self.split.clone() {
            Some(tree) if tree.contains(pane) && self.is_split() => tree,
            // The pane on screen is a tab outside the split: it starts a new one.
            _ => PaneTree::Leaf(pane),
        };
        tree.place(pane, id, edge).then_some(tree)
    }

    /// Drops tab `id` on `target` (see [`App::arranged`]). The dropped tab takes the keyboard.
    pub fn drop_tab(&mut self, id: u64, target: DropTarget) {
        match target {
            DropTarget::Strip(index) => self.move_tab(id, index),
            DropTarget::Pane(pane, edge) => {
                if let Some(tree) = self.arranged(id, pane, edge) {
                    self.set_split(Some(tree));
                    self.select(id);
                }
            }
        }
    }

    /// Moves tab `id` to before the tab now at `index` (the end for `len`). A flat split
    /// follows the strip's order.
    pub fn move_tab(&mut self, id: u64, index: usize) {
        let Some(from) = self.sessions.iter().position(|s| s.id == id) else {
            return;
        };
        let to = if index > from { index - 1 } else { index };
        let session = self.sessions.remove(from);
        self.sessions.insert(to.min(self.sessions.len()), session);
        let order = self.ids();
        if let Some(tree) = &mut self.split {
            tree.follow_order(&order);
        }
    }

    /// The divider between children `index` and `index + 1` of the split at `path` is dragged:
    /// the two panes trade space and the rest keep theirs.
    pub fn drag_divider(&mut self, path: &[usize], index: usize, d: &Drag) {
        let Some(tree) = &mut self.split else { return };
        if d.phase == Phase::Down {
            self.divider_drag = match tree.node(path) {
                Some(PaneTree::Split { children, .. }) if index + 1 < children.len() => {
                    Some(DividerDrag {
                        path: path.to_vec(),
                        index,
                        start: (children[index].1, children[index + 1].1),
                    })
                }
                _ => None,
            };
            return;
        }
        let Some(drag) = self.divider_drag.clone() else {
            return;
        };
        let Some(PaneTree::Split { axis, children }) = tree.node_mut(&drag.path) else {
            return;
        };
        let total: f32 = children.iter().map(|(_, w)| w).sum();
        let dividers = DIVIDER * children.len().saturating_sub(1) as f32;
        let length = self.rects.splits.get(&drag.path).copied().unwrap_or(0.0) - dividers;
        let delta = if *axis == Axis::Row { d.dx } else { d.dy };
        let (a, b) = if d.phase == Phase::Up && d.cancelled() {
            drag.start
        } else {
            panes::drag_weights(drag.start, total, length, delta)
        };
        children[drag.index].1 = a;
        children[drag.index + 1].1 = b;
        if d.phase == Phase::Up {
            self.divider_drag = None;
        }
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
        // Opened from a split pane, the tab joins the split beside it.
        if self.is_split()
            && let (Some(tree), Some(focused)) = (&mut self.split, self.selected)
        {
            tree.add_beside(focused, session.id);
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
        let split = self.split.take().and_then(|t| t.without(id));
        self.set_split(split);
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
        self.select(self.sessions[next].id);
    }

    // MARK: Layout

    /// The selected tab's grid size (or, with no tab, a new tab's).
    pub fn grid_size(&self) -> Option<GridSize> {
        match self.selected() {
            Some(session) => self.grid_size_for(session),
            None => self.grid_size_in(self.pane_size, &self.current_appearance()),
        }
    }

    /// The grid that fits `session`'s pane: its own when split (once measured), else the
    /// selected pane's, which every tab shares while one tab is on screen.
    pub fn grid_size_for(&self, session: &Session) -> Option<GridSize> {
        let own = session.pane_size;
        let pane = if self.is_split() && own.0 > 0.0 && own.1 > 0.0 {
            own
        } else {
            self.pane_size
        };
        self.grid_size_in(pane, &session.appearance)
    }

    fn grid_size_in(&self, (w, h): (f32, f32), a: &TerminalAppearance) -> Option<GridSize> {
        let cell = self.cell?;
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

    /// Tab `id`'s pane measured `w` × `h` points of terminal area.
    ///
    /// Every tab is fitted again, not only on a change: a pane back on screen at its old size
    /// may hold a grid resized for one tab at a time meanwhile.
    pub fn set_pane_size(&mut self, id: u64, w: f32, h: f32) {
        if let Some(session) = self.sessions.iter_mut().find(|s| s.id == id) {
            session.pane_size = (w, h);
        }
        if Some(id) == self.selected {
            self.pane_size = (w, h);
        }
        self.resize_all();
    }

    fn resize_all(&self) {
        for session in &self.sessions {
            if let Some(size) = self.grid_size_for(session) {
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
        let on_screen = self.visible_ids();
        let mut notes = Vec::new();
        for session in &mut self.sessions {
            let visible = on_screen.contains(&session.id);
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
                if long && !self.visible_ids().contains(&id) {
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
        if self.terminal_menu.is_some() && ev.key == rune::Key::Escape {
            self.terminal_menu = None;
            return true;
        }
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

    /// A wheel or trackpad scroll over tab `id`'s grid area, at (`x`, `y`) in its own points:
    /// whole lines go to that tab (scrollback, or the program on the alternate screen), focused
    /// or not, as in other terminals' split panes.
    pub fn on_wheel(&mut self, id: u64, ev: WheelEvent) -> bool {
        if ev.dy == 0.0 {
            return false;
        }
        let Some(index) = self.sessions.iter().position(|s| s.id == id) else {
            return false;
        };
        let cell = self
            .cell
            .unwrap_or_else(|| crate::ui::grid::measure_cell(&self.sessions[index].appearance));
        let step = if ev.precise {
            ev.dy / cell.height.max(1.0)
        } else {
            ev.dy / WHEEL_NOTCH_POINTS * WHEEL_NOTCH_LINES
        };
        let lines = take_whole_lines(&mut self.wheel_rest, step);
        if lines != 0 {
            let session = &self.sessions[index];
            let a = &session.appearance;
            let size = session.engine().size();
            let col = cell_index(ev.x - a.padding_horizontal, cell.width, size.cols);
            let row = cell_index(ev.y - a.padding_vertical, cell.height, size.rows);
            let mods = Modifiers {
                shift: ev.mods.shift,
                control: ev.mods.ctrl,
                alt: ev.mods.alt,
            };
            session.wheel(lines, col, row, mods);
        }
        true
    }

    /// A press or drag over tab `id`'s grid area, at the drag's (`x`, `y`) in its own points:
    /// a click starts a selection, a double click selects a word and a triple click a line, and
    /// dragging extends it by the same unit. Shift-click extends the selection there is.
    pub fn on_grid_drag(&mut self, id: u64, drag: &Drag, shift: bool) {
        let Some(index) = self.sessions.iter().position(|s| s.id == id) else {
            return;
        };
        if drag.phase == Phase::Down {
            self.terminal_menu = None;
            self.select(id);
        }
        let cell = self
            .cell
            .unwrap_or_else(|| crate::ui::grid::measure_cell(&self.sessions[index].appearance));
        let session = &self.sessions[index];
        let a = &session.appearance;
        let size = session.engine().size();
        let x = drag.x - a.padding_horizontal;
        let col = cell_index(x, cell.width, size.cols);
        let right_half = x > (col as f32 + 0.5) * cell.width;
        let row = session.engine().top_visible_row()
            + cell_index(drag.y - a.padding_vertical, cell.height, size.rows);
        let engine = session.engine();
        match drag.phase {
            Phase::Down if shift => engine.extend_selection(row, col, right_half),
            Phase::Down => {
                let kind = match drag.clicks {
                    0 | 1 => SelectionKind::Cells,
                    2 => SelectionKind::Words,
                    _ => SelectionKind::Lines,
                };
                engine.begin_selection(kind, row, col, right_half);
            }
            Phase::Move => engine.extend_selection(row, col, right_half),
            Phase::Up => {}
        }
    }

    /// Copy: the selection, or with none the visible screen.
    pub fn copy(&mut self, tx: &mut Tx) {
        let Some(s) = self.selected() else {
            return;
        };
        let text = s.engine().selection_text().unwrap_or_else(|| {
            let top = s.engine().top_visible_row();
            let rows = s.engine().size().rows as usize;
            TerminalEngine::text(s.engine(), top, top + rows)
        });
        tx.copy(text);
    }

    pub fn run_shortcut(&mut self, shortcut: Shortcut, tx: &mut Tx) {
        self.terminal_menu = None;
        match shortcut {
            Shortcut::NewTab => self.new_tab(),
            Shortcut::CloseTab => {
                if let Some(id) = self.selected {
                    self.close(id);
                }
            }
            Shortcut::CloseWindow => tx.close_window(),
            Shortcut::CommandPalette | Shortcut::OpenHost => self.open_palette(tx),
            Shortcut::ScrollPageUp => self.with_selected(|s| s.scroll_page(true)),
            Shortcut::ScrollPageDown => self.with_selected(|s| s.scroll_page(false)),
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
            Shortcut::Copy => self.copy(tx),
            Shortcut::SelectAll => self.with_selected(|s| s.engine().select_all()),
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
            Shortcut::SplitSideBySide => self.toggle_layout(Layout::SideBySide),
            Shortcut::SplitStacked => self.toggle_layout(Layout::Stacked),
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
        palette::rank(
            palette::all_items(self.layout(), &hosts, &tabs, &recent),
            query,
        )
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
            PaletteAction::SetLayout(layout) => self.set_layout(layout),
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

/// A divider between split panes, in points along the split.
pub const DIVIDER: f32 = 5.0;

/// Points Rune reports for one notch of a mouse wheel (its `LINE_POINTS`): winit's line delta
/// scaled, larger on Linux.
const WHEEL_NOTCH_POINTS: f32 = if cfg!(target_os = "linux") {
    120.0
} else {
    40.0
};
/// Lines one wheel notch scrolls, as in Terminal.app, xterm and Alacritty.
const WHEEL_NOTCH_LINES: f32 = 3.0;

/// Adds `step` lines of travel to `rest` and takes out the whole lines, keeping the fraction.
/// Reversing direction drops the leftover, so a turn never starts with a stale partial line.
fn take_whole_lines(rest: &mut f32, step: f32) -> i32 {
    if *rest != 0.0 && rest.signum() != step.signum() {
        *rest = 0.0;
    }
    *rest += step;
    let whole = rest.trunc();
    *rest -= whole;
    whole as i32
}

/// The 0-based cell under `offset` points into the grid, clamped onto it.
fn cell_index(offset: f32, cell: f32, count: u16) -> usize {
    let last = usize::from(count.max(1) - 1);
    if offset <= 0.0 || cell <= 0.0 {
        return 0;
    }
    ((offset / cell) as usize).min(last)
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
    fn take_whole_lines_should_carry_trackpad_fractions_until_a_line_is_due() {
        let mut rest = 0.0;
        assert_eq!(take_whole_lines(&mut rest, 0.4), 0);
        assert_eq!(take_whole_lines(&mut rest, 0.4), 0);
        assert_eq!(take_whole_lines(&mut rest, 0.4), 1);
        assert!((rest - 0.2).abs() < 1e-5);
    }

    #[test]
    fn take_whole_lines_should_drop_the_leftover_when_the_direction_turns() {
        let mut rest = 0.0;
        assert_eq!(take_whole_lines(&mut rest, 0.9), 0);
        assert_eq!(take_whole_lines(&mut rest, -0.5), 0);
        assert!((rest + 0.5).abs() < 1e-5);
        assert_eq!(take_whole_lines(&mut rest, -2.5), -3);
    }

    #[test]
    fn a_mouse_wheel_notch_should_scroll_three_lines() {
        let mut rest = 0.0;
        let step = WHEEL_NOTCH_POINTS / WHEEL_NOTCH_POINTS * WHEEL_NOTCH_LINES;
        assert_eq!(take_whole_lines(&mut rest, step), 3);
        assert_eq!(take_whole_lines(&mut rest, -step), -3);
    }

    #[test]
    fn cell_index_should_clamp_onto_the_grid() {
        assert_eq!(cell_index(-4.0, 8.0, 80), 0);
        assert_eq!(cell_index(17.0, 8.0, 80), 2);
        assert_eq!(cell_index(10_000.0, 8.0, 80), 79);
    }

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
    fn asking_for_the_current_layout_should_go_back_to_one_tab() {
        assert_eq!(
            Layout::Single.toggled(Layout::SideBySide),
            Layout::SideBySide
        );
        assert_eq!(Layout::SideBySide.toggled(Layout::Stacked), Layout::Stacked);
        assert_eq!(Layout::Stacked.toggled(Layout::Stacked), Layout::Single);
    }

    #[test]
    fn the_rail_button_should_cycle_through_every_layout() {
        let cycle: Vec<Layout> = std::iter::successors(Some(Layout::Single), |l| Some(l.next()))
            .take(4)
            .collect();
        assert_eq!(
            cycle,
            [
                Layout::Single,
                Layout::SideBySide,
                Layout::Stacked,
                Layout::Single
            ]
        );
        assert_eq!(Layout::Custom.next(), Layout::Single);
    }

    #[test]
    fn app_should_close_the_window_only_after_tabs_existed() {
        let app = App::new(ThemeLibrary::built_in());
        assert!(!app.had_tabs && app.sessions.is_empty());
    }
}
