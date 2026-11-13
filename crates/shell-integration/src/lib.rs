//! Shell integration: OSC scanning, the command timeline, and dotfile-free shell injection.
//!
//! Port of the Swift `ShellIntegration` module.

mod command_timeline;
mod installer;
mod osc;

pub use command_timeline::{CommandId, CommandRecord, CommandStatus, CommandTimeline};
pub use installer::{InstallerError, LaunchPlan, Shell, ShellIntegrationInstaller};
pub use osc::{FileUrl, OscScanner, ShellEvent};
