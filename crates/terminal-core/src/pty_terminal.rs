use std::io::{Read, Write};
use std::ops::Range;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};
use std::thread;
use std::time::Instant;

use alacritty_terminal::Term;
use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::term::Config;
use alacritty_terminal::vte::ansi::{self, CursorShape, CursorStyle, Rgb};
use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};
use shell_integration::{LaunchPlan, OscScanner};

use crate::snapshot::{self, Palette, TerminalModes};
use crate::{
    EngineError, EngineEvent, GridSize, GridSnapshot, Key, Modifiers, TerminalAppearance,
    TerminalCursorShape, TerminalEngine, Waker, encode_key, encode_paste,
};

/// Construction options for [`PtyTerminal`].
#[derive(Debug, Clone, PartialEq)]
pub struct PtyTerminalOptions {
    /// Initial grid size.
    pub size: GridSize,
    /// Maximum scrollback lines.
    pub scrollback: usize,
    /// Initial appearance.
    pub appearance: TerminalAppearance,
}

impl Default for PtyTerminalOptions {
    fn default() -> Self {
        Self {
            size: GridSize::default(),
            scrollback: 10_000,
            appearance: TerminalAppearance::default(),
        }
    }
}

impl Dimensions for GridSize {
    fn total_lines(&self) -> usize {
        usize::from(self.rows)
    }

    fn screen_lines(&self) -> usize {
        usize::from(self.rows)
    }

    fn columns(&self) -> usize {
        usize::from(self.cols)
    }
}

type ColorRequest = (usize, Arc<dyn Fn(Rgb) -> String + Sync + Send + 'static>);

/// State shared by the engine handle, the emulator's event listener and the I/O threads.
/// Lock order: `term` before any lock in here.
struct Outbox {
    events: Sender<EngineEvent>,
    input: Mutex<Option<Sender<Vec<u8>>>>,
    appearance: RwLock<TerminalAppearance>,
    size: Mutex<GridSize>,
    waker: Mutex<Option<Waker>>,
    color_requests: Mutex<Vec<ColorRequest>>,
}

impl Outbox {
    fn emit(&self, event: EngineEvent) {
        // The receiver may be gone (UI closed the pane); nothing to do then.
        let _ = self.events.send(event);
    }

    fn write(&self, bytes: Vec<u8>) -> bool {
        lock(&self.input)
            .as_ref()
            .is_some_and(|tx| tx.send(bytes).is_ok())
    }

    fn wake(&self) {
        let waker = lock(&self.waker).clone();
        if let Some(waker) = waker {
            waker();
        }
    }

    fn appearance(&self) -> TerminalAppearance {
        self.appearance
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Receives alacritty's callbacks; runs with the terminal locked, so it never touches `term`.
#[derive(Clone)]
struct Listener(Arc<Outbox>);

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        let outbox = &self.0;
        match event {
            Event::Title(title) => outbox.emit(EngineEvent::Title(title)),
            Event::ResetTitle => outbox.emit(EngineEvent::Title(String::new())),
            Event::Bell => outbox.emit(EngineEvent::Bell),
            Event::ClipboardStore(_, text) => outbox.emit(EngineEvent::ClipboardStore(text)),
            Event::PtyWrite(text) => {
                outbox.write(text.into_bytes());
            }
            // Needs the terminal's own color overrides: answered by the reader after the chunk.
            Event::ColorRequest(index, format) => {
                lock(&outbox.color_requests).push((index, format))
            }
            Event::TextAreaSizeRequest(format) => {
                let size = *lock(&outbox.size);
                let window = WindowSize {
                    num_lines: size.rows,
                    num_cols: size.cols,
                    cell_width: 0,
                    cell_height: 0,
                };
                outbox.write(format(window).into_bytes());
            }
            // Clipboard reads (OSC 52 paste) are refused: output must not read the pasteboard.
            Event::ClipboardLoad(..)
            | Event::MouseCursorDirty
            | Event::CursorBlinkingChange
            | Event::Wakeup
            | Event::Exit
            | Event::ChildExit(_) => {}
        }
    }
}

struct Running {
    master: Mutex<Box<dyn MasterPty + Send>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    pid: Option<u32>,
    master_fd: Option<i32>,
}

/// The terminal engine: a shell on a PTY, emulated by `alacritty_terminal`.
///
/// A reader thread feeds PTY output to both the emulator and an [`OscScanner`], interleaved so
/// every [`EngineEvent::Shell`] carries the exact cursor row, then calls the [`Waker`].
/// Input goes through a writer thread, so [`send`](TerminalEngine::send) never blocks.
pub struct PtyTerminal {
    term: Arc<Mutex<Term<Listener>>>,
    outbox: Arc<Outbox>,
    running: Option<Running>,
    scrollback: usize,
}

impl PtyTerminal {
    /// Creates an idle engine and the receiver for its events. Call [`start`](TerminalEngine::start) next.
    pub fn new(options: PtyTerminalOptions) -> (Self, Receiver<EngineEvent>) {
        let (events, receiver) = mpsc::channel();
        let size = GridSize::new(options.size.cols, options.size.rows);
        let config = Config {
            scrolling_history: options.scrollback,
            default_cursor_style: cursor_style(&options.appearance),
            ..Config::default()
        };
        let outbox = Arc::new(Outbox {
            events,
            input: Mutex::new(None),
            appearance: RwLock::new(options.appearance),
            size: Mutex::new(size),
            waker: Mutex::new(None),
            color_requests: Mutex::new(Vec::new()),
        });
        let term = Term::new(config, &size, Listener(Arc::clone(&outbox)));
        (
            Self {
                term: Arc::new(Mutex::new(term)),
                outbox,
                running: None,
                scrollback: options.scrollback,
            },
            receiver,
        )
    }

    /// Sets (or clears) the redraw callback; invoked from the reader thread.
    pub fn set_waker(&self, waker: Option<Waker>) {
        *lock(&self.outbox.waker) = waker;
    }

    fn term(&self) -> MutexGuard<'_, Term<Listener>> {
        lock(&self.term)
    }

    /// Current grid size.
    pub fn size(&self) -> GridSize {
        *lock(&self.outbox.size)
    }

    /// The visible viewport (honoring scrollback position).
    pub fn snapshot(&self) -> GridSnapshot {
        let appearance = self.outbox.appearance();
        let term = self.term();
        let top = snapshot::top_visible_row(&term);
        snapshot::build(&term, top..top + term.grid().screen_lines(), &appearance)
    }

    /// Arbitrary absolute rows (e.g. for a minimap or search results).
    pub fn snapshot_rows(&self, rows: Range<usize>) -> GridSnapshot {
        let appearance = self.outbox.appearance();
        snapshot::build(&self.term(), rows, &appearance)
    }

    /// Input-relevant terminal modes (DECCKM, bracketed paste, …).
    pub fn modes(&self) -> TerminalModes {
        TerminalModes::from_term(*self.term().mode())
    }

    /// Encodes a key press for the current modes and sends it.
    pub fn send_key(&self, key: &Key, mods: Modifiers) -> Result<(), EngineError> {
        let bytes = encode_key(key, mods, self.modes().app_cursor);
        if bytes.is_empty() {
            Ok(())
        } else {
            self.send(&bytes)
        }
    }

    /// Sends a paste, bracketed if the program enabled it.
    pub fn paste(&self, text: &str) -> Result<(), EngineError> {
        self.send(&encode_paste(text, self.modes().bracketed_paste))
    }

    /// Scrolls the viewport by `delta` lines (positive = back into history).
    pub fn scroll_lines(&self, delta: i32) {
        self.term().scroll_display(Scroll::Delta(delta));
    }

    /// Processes `bytes` as if the PTY had produced them, without a shell: demo sessions and
    /// headless snapshots. OSC 133 marks become [`EngineEvent::Shell`] events as usual.
    pub fn feed(&self, bytes: &[u8]) {
        let mut state = ReaderState {
            term: Arc::clone(&self.term),
            outbox: Arc::clone(&self.outbox),
            processor: ansi::Processor::new(),
            scanner: OscScanner::new(),
            master_fd: None,
        };
        state.process(bytes);
    }

    /// Returns to the live screen.
    pub fn scroll_to_bottom(&self) {
        self.term().scroll_display(Scroll::Bottom);
    }

    fn spawn(&mut self, plan: &LaunchPlan) -> Result<(), EngineError> {
        if !is_executable(Path::new(&plan.executable)) {
            return Err(EngineError::ShellNotExecutable(plan.executable.clone()));
        }
        let size = self.size();
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: size.rows,
                cols: size.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| EngineError::Pty(e.to_string()))?;
        let child = pair
            .slave
            .spawn_command(command(plan))
            .map_err(|e| EngineError::Pty(e.to_string()))?;
        // The parent must not keep the slave open, or the reader never sees EOF when the shell exits.
        drop(pair.slave);

        let master = pair.master;
        let reader = master
            .try_clone_reader()
            .map_err(|e| EngineError::Pty(e.to_string()))?;
        let writer = master
            .take_writer()
            .map_err(|e| EngineError::Pty(e.to_string()))?;
        let master_fd = master_fd(master.as_ref());
        let pid = child.process_id();
        let killer = child.clone_killer();

        let (input_tx, input_rx) = mpsc::channel::<Vec<u8>>();
        thread::Builder::new()
            .name("glasshouse-pty-writer".into())
            .spawn(move || write_loop(writer, input_rx))?;
        *lock(&self.outbox.input) = Some(input_tx);

        let reader_state = ReaderState {
            term: Arc::clone(&self.term),
            outbox: Arc::clone(&self.outbox),
            processor: ansi::Processor::new(),
            scanner: OscScanner::new(),
            master_fd,
        };
        thread::Builder::new()
            .name("glasshouse-pty-reader".into())
            .spawn(move || {
                reader_state.run(reader, child);
            })?;

        self.running = Some(Running {
            master: Mutex::new(master),
            killer: Mutex::new(killer),
            pid,
            master_fd,
        });
        Ok(())
    }
}

impl TerminalEngine for PtyTerminal {
    fn start(&mut self, plan: &LaunchPlan) -> Result<(), EngineError> {
        if self.running.is_some() {
            return Err(EngineError::AlreadyStarted);
        }
        self.spawn(plan)
    }

    fn send(&self, bytes: &[u8]) -> Result<(), EngineError> {
        if self.outbox.write(bytes.to_vec()) {
            Ok(())
        } else {
            Err(EngineError::NotRunning)
        }
    }

    fn resize(&self, size: GridSize) -> Result<(), EngineError> {
        let size = GridSize::new(size.cols, size.rows);
        if let Some(running) = &self.running {
            lock(&running.master)
                .resize(PtySize {
                    rows: size.rows,
                    cols: size.cols,
                    pixel_width: 0,
                    pixel_height: 0,
                })
                .map_err(|e| EngineError::Pty(e.to_string()))?;
        }
        {
            let mut term = self.term();
            term.resize(size);
            *lock(&self.outbox.size) = size;
        }
        self.outbox.emit(EngineEvent::Resized {
            cols: size.cols,
            rows: size.rows,
        });
        self.outbox.wake();
        Ok(())
    }

    fn terminate(&self) {
        let Some(running) = &self.running else { return };
        #[cfg(unix)]
        if let Some(pid) = running
            .pid
            .and_then(|p| libc::pid_t::try_from(p).ok())
            .filter(|p| *p > 0)
        {
            // SAFETY: kill(2) has no memory-safety preconditions. SIGHUP, like closing a Terminal.app tab.
            unsafe { libc::kill(pid, libc::SIGHUP) };
            return;
        }
        let _ = lock(&running.killer).kill();
    }

    fn pty_master_fd(&self) -> Option<i32> {
        self.running.as_ref().and_then(|r| r.master_fd)
    }

    fn shell_pid(&self) -> Option<u32> {
        self.running.as_ref().and_then(|r| r.pid)
    }

    fn cursor_absolute_row(&self) -> usize {
        let term = self.term();
        snapshot::absolute_row(&term, term.grid().cursor.point.line)
    }

    fn total_rows(&self) -> usize {
        self.term().grid().total_lines()
    }

    fn top_visible_row(&self) -> usize {
        snapshot::top_visible_row(&self.term())
    }

    fn scroll_to_absolute_row(&self, row: usize) {
        let mut term = self.term();
        let history = term.grid().history_size();
        let desired_offset = history.saturating_sub(row);
        let current = term.grid().display_offset();
        let delta =
            i64::try_from(desired_offset).unwrap_or(i64::MAX) - i64::try_from(current).unwrap_or(0);
        term.scroll_display(Scroll::Delta(i32::try_from(delta).unwrap_or(0)));
    }

    fn text(&self, from_row: usize, to_row: usize) -> String {
        if to_row <= from_row {
            return String::new();
        }
        snapshot::text(&self.term(), from_row, to_row)
    }

    fn apply(&self, appearance: &TerminalAppearance) {
        *self
            .outbox
            .appearance
            .write()
            .unwrap_or_else(PoisonError::into_inner) = appearance.clone();
        let mut term = self.term();
        let config = Config {
            scrolling_history: self.scrollback,
            default_cursor_style: cursor_style(appearance),
            ..Config::default()
        };
        term.set_options(config);
        drop(term);
        self.outbox.wake();
    }
}

impl Drop for PtyTerminal {
    fn drop(&mut self) {
        // Closing the input channel ends the writer thread; dropping the master hangs up the shell.
        *lock(&self.outbox.input) = None;
    }
}

fn cursor_style(appearance: &TerminalAppearance) -> CursorStyle {
    let shape = match appearance.cursor_shape {
        TerminalCursorShape::Block => CursorShape::Block,
        TerminalCursorShape::Underline => CursorShape::Underline,
        TerminalCursorShape::Bar => CursorShape::Beam,
    };
    CursorStyle {
        shape,
        blinking: appearance.cursor_blink,
    }
}

fn command(plan: &LaunchPlan) -> CommandBuilder {
    let name = Path::new(&plan.executable)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(&plan.executable);
    let login = plan.args.is_empty() && plan.exec_name == format!("-{name}");
    // portable-pty can only set a custom argv[0] for its "default program", which execs $SHELL as
    // a login shell with argv[0] = "-<basename>" — exactly the login case. Otherwise argv[0] is the path.
    let mut cmd = if login {
        CommandBuilder::new_default_prog()
    } else {
        CommandBuilder::new(&plan.executable)
    };
    cmd.args(&plan.args);
    cmd.env_clear();
    for (key, value) in &plan.environment {
        cmd.env(key, value);
    }
    if login {
        cmd.env("SHELL", &plan.executable);
    }
    cmd
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.is_absolute()
        && std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

#[cfg(unix)]
fn master_fd(master: &dyn MasterPty) -> Option<i32> {
    master.as_raw_fd()
}

#[cfg(not(unix))]
fn master_fd(_master: &dyn MasterPty) -> Option<i32> {
    None
}

fn write_loop(mut writer: Box<dyn Write + Send>, input: Receiver<Vec<u8>>) {
    for bytes in input {
        if writer
            .write_all(&bytes)
            .and_then(|()| writer.flush())
            .is_err()
        {
            break;
        }
    }
}

struct ReaderState {
    term: Arc<Mutex<Term<Listener>>>,
    outbox: Arc<Outbox>,
    processor: ansi::Processor,
    scanner: OscScanner,
    master_fd: Option<i32>,
}

impl ReaderState {
    fn run(
        mut self,
        mut reader: Box<dyn Read + Send>,
        mut child: Box<dyn portable_pty::Child + Send + Sync>,
    ) {
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            // A synchronized update (DECSET 2026) buffers output until it ends or times out; alacritty
            // leaves the timeout to the event loop, which is us.
            if let Some(deadline) = self.processor.sync_timeout().sync_timeout()
                && !self.wait_readable(deadline)
            {
                self.processor.stop_sync(&mut *lock(&self.term));
                self.outbox.emit(EngineEvent::OutputActivity);
                self.outbox.wake();
                continue;
            }
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => self.process(&buf[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                // EIO on Linux once the shell side closes.
                Err(_) => break,
            }
        }
        *lock(&self.outbox.input) = None;
        let code = child.wait().ok().and_then(|status| {
            if status.signal().is_some() {
                None
            } else {
                i32::try_from(status.exit_code()).ok()
            }
        });
        self.outbox.emit(EngineEvent::Exited { code });
        self.outbox.wake();
    }

    fn process(&mut self, chunk: &[u8]) {
        let mut shell_events = Vec::new();
        {
            let mut term = lock(&self.term);
            let mut start = 0;
            for (i, &byte) in chunk.iter().enumerate() {
                if let Some(event) = self.scanner.scan_byte(byte) {
                    self.processor.advance(&mut *term, &chunk[start..=i]);
                    start = i + 1;
                    let row = snapshot::absolute_row(&term, term.grid().cursor.point.line);
                    shell_events.push(EngineEvent::Shell { event, row });
                }
            }
            self.processor.advance(&mut *term, &chunk[start..]);
            self.answer_color_requests(&term);
        }
        for event in shell_events {
            self.outbox.emit(event);
        }
        self.outbox.emit(EngineEvent::OutputActivity);
        self.outbox.wake();
    }

    fn answer_color_requests(&self, term: &Term<Listener>) {
        let requests = std::mem::take(&mut *lock(&self.outbox.color_requests));
        if requests.is_empty() {
            return;
        }
        let appearance = self.outbox.appearance();
        let palette = Palette {
            colors: term.colors(),
            appearance: &appearance,
        };
        for (index, format) in requests {
            let [r, g, b] = palette.index(index).to_rgb8();
            self.outbox.write(format(Rgb { r, g, b }).into_bytes());
        }
    }

    /// Waits until the PTY is readable or `deadline` passes; true if readable.
    #[cfg(unix)]
    fn wait_readable(&self, deadline: Instant) -> bool {
        let Some(fd) = self.master_fd else {
            return true;
        };
        let remaining = deadline.saturating_duration_since(Instant::now());
        let timeout = i32::try_from(remaining.as_millis()).unwrap_or(i32::MAX);
        let mut pfd = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: `pfd` is a valid pollfd array of length 1 for the duration of the call.
        let n = unsafe { libc::poll(&mut pfd, 1, timeout) };
        n != 0
    }

    #[cfg(not(unix))]
    fn wait_readable(&self, _deadline: Instant) -> bool {
        true
    }
}
