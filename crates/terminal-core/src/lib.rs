//! Terminal core: PTY environment, password-prompt detection, and the terminal engine.
//!
//! Port of the Swift `TerminalCore` module. The engine ([`PtyTerminal`]) runs the shell on a
//! PTY (portable-pty), emulates it with `alacritty_terminal`, and taps the byte stream with
//! [`shell_integration::OscScanner`] for OSC 133/7/9/777 events. The UI renders from
//! [`GridSnapshot`]s and encodes keyboard input with [`encode_key`].

mod appearance;
mod engine;
mod environment;
mod keys;
mod password;
mod pty_terminal;
mod snapshot;

pub use appearance::{TerminalAppearance, TerminalCursorShape};
pub use design_system::Rgba;
pub use engine::{EngineError, EngineEvent, GridSize, TerminalEngine, Waker};
pub use environment::{PtyEnvironment, PtyEnvironmentOptions};
pub use keys::{Key, Modifiers, encode_key, encode_mouse_wheel, encode_paste};
pub use password::{PasswordPromptDetector, PasswordPromptState};
pub use pty_terminal::{PtyTerminal, PtyTerminalOptions, SelectionKind};
pub use snapshot::{
    CellFlags, CellSpan, CursorSnapshot, GridCell, GridLine, GridSnapshot, SelectionBounds,
    TerminalModes,
};
