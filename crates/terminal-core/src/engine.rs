use std::sync::Arc;

use shell_integration::{LaunchPlan, ShellEvent};

use crate::TerminalAppearance;

/// Called (from the engine's reader thread) after events are queued or output changed the grid,
/// so a UI can schedule a redraw and drain its event receiver.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// Grid size in cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridSize {
    /// Columns.
    pub cols: u16,
    /// Rows (screen lines).
    pub rows: u16,
}

impl GridSize {
    /// A size, clamped to at least 2 columns × 1 row.
    pub fn new(cols: u16, rows: u16) -> Self {
        Self {
            cols: cols.max(2),
            rows: rows.max(1),
        }
    }
}

impl Default for GridSize {
    fn default() -> Self {
        Self::new(80, 24)
    }
}

/// Engine → session events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineEvent {
    /// A shell-integration event, with the absolute row of the cursor at the moment the
    /// sequence was parsed (exact: emulation and scanning are interleaved byte-for-byte).
    Shell {
        /// The event.
        event: ShellEvent,
        /// Absolute row (scrollback + screen line).
        row: usize,
    },
    /// Window title set by OSC 0/2 (empty when a program resets it).
    Title(String),
    /// The grid was resized.
    Resized {
        /// Columns.
        cols: u16,
        /// Rows.
        rows: u16,
    },
    /// Output arrived. The session uses this as the cue to re-check termios (cheap ioctl, no polling).
    OutputActivity,
    /// BEL.
    Bell,
    /// A program stored text to the clipboard with OSC 52.
    ClipboardStore(String),
    /// The shell exited; `None` if it was killed by a signal.
    Exited {
        /// Exit status.
        code: Option<i32>,
    },
}

/// Engine failures.
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    /// `start` was called twice.
    #[error("the terminal is already running a shell — create a new engine for another session")]
    AlreadyStarted,
    /// Input was sent before `start` or after the shell exited.
    #[error("the terminal has no running shell — call start() first")]
    NotRunning,
    /// The shell path is missing or not executable.
    #[error("shell {0:?} does not exist or is not executable — check the configured shell path")]
    ShellNotExecutable(String),
    /// Opening the PTY, spawning or resizing failed.
    #[error("PTY operation failed: {0}")]
    Pty(String),
    /// Spawning the reader/writer thread failed.
    #[error("failed to spawn the terminal I/O thread: {0}")]
    Thread(#[from] std::io::Error),
}

/// Everything the UI needs from a terminal engine.
///
/// Rows are *absolute*: history size + screen line, so row 0 is the oldest scrollback line.
/// Marks stay valid while scrollback grows; once history reaches its limit (or on the
/// alternate screen, which has no history) rows shift with trimming.
pub trait TerminalEngine: Send {
    /// Spawns the shell described by `plan` on a new PTY.
    fn start(&mut self, plan: &LaunchPlan) -> Result<(), EngineError>;
    /// Writes raw bytes to the PTY (keyboard input, paste). Never blocks.
    fn send(&self, bytes: &[u8]) -> Result<(), EngineError>;
    /// Writes UTF-8 text to the PTY.
    fn send_text(&self, text: &str) -> Result<(), EngineError> {
        self.send(text.as_bytes())
    }
    /// Resizes the PTY and the grid.
    fn resize(&self, size: GridSize) -> Result<(), EngineError>;
    /// Sends SIGHUP to the shell, like closing a Terminal.app tab.
    fn terminate(&self);

    /// PTY master fd — used for termios password-prompt detection and foreground-process lookup.
    fn pty_master_fd(&self) -> Option<i32>;
    /// The shell's process id.
    fn shell_pid(&self) -> Option<u32>;

    /// Absolute row of the cursor; anchors OSC 133 marks.
    fn cursor_absolute_row(&self) -> usize;
    /// Total rows (scrollback + visible) — for mapping marks onto the scrollbar gutter.
    fn total_rows(&self) -> usize;
    /// First visible absolute row.
    fn top_visible_row(&self) -> usize;
    /// Scrolls so `row` is the first visible row (clamped).
    fn scroll_to_absolute_row(&self, row: usize);
    /// Plain text of absolute rows `[from_row, to_row)`, trailing whitespace trimmed per line,
    /// joined with `\n`. Used by "Copy output" and command-line capture.
    fn text(&self, from_row: usize, to_row: usize) -> String;

    /// Applies colors and cursor defaults.
    fn apply(&self, appearance: &TerminalAppearance);
}
