//! Terminal themes for Glasshouse: colours, the YAML theme format, `appearance.yaml`, the theme store and importers
//! for iTerm2 and Ghostty themes. Ported from the Swift `TerminalTheme` module.
//!
//! Start with [`ThemeStore`]: it lists the built-in and user themes, saves, duplicates, deletes and imports them,
//! and resolves the theme and settings for the current system appearance into a [`ResolvedAppearance`].

mod appearance_config;
mod terminal_theme;
mod theme_color;
mod theme_importer;
mod theme_store;
mod yaml;

pub use appearance_config::{AppearanceConfig, ResolvedAppearance, ThemeChoice};
pub use terminal_theme::{
    AnsiColors, Appearance, ContrastLevel, CursorSettings, CursorShape, FontSettings, Palette,
    TerminalTheme, ThemeError, WindowSettings,
};
pub use theme_color::ThemeColor;
pub use theme_importer::ThemeImporter;
pub use theme_store::{ThemeFileProblem, ThemeStore, default_support_directory};
