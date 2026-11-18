//! `appearance.yaml`: which themes to use, and the settings every theme starts from.

use std::ops::RangeInclusive;

use crate::{
    ContrastLevel, CursorSettings, CursorShape, FontSettings, TerminalTheme, ThemeError,
    WindowSettings,
};

/// Which theme to use in each system appearance. YAML section `theme`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeChoice {
    /// Theme id for Dark Mode.
    pub dark: String,
    /// Theme id for Light Mode.
    pub light: String,
    /// Use `light` in Light Mode and `dark` in Dark Mode. When false, `dark` is used all the time. YAML key
    /// `follow-system`.
    pub follow_system: bool,
}

impl Default for ThemeChoice {
    fn default() -> Self {
        Self {
            dark: "glasshouse-dark".into(),
            light: "glasshouse-light".into(),
            follow_system: true,
        }
    }
}

impl ThemeChoice {
    /// The theme id to use when the system is (`is_dark`) or isn't in Dark Mode.
    pub fn theme_id(&self, is_dark: bool) -> &str {
        if self.follow_system && !is_dark {
            &self.light
        } else {
            &self.dark
        }
    }
}

/// `appearance.yaml`: which themes to use, and the settings every theme starts from. Every key may be missing in
/// the file; a missing one keeps its default.
///
/// ```yaml
/// schema: 1
/// theme:
///   dark: dracula
///   light: solarized-light
///   follow-system: true      # false: always use the dark theme
/// font: { family: SF Mono, size: 13, bold-is-bright: true }
/// cursor: { style: block, blink: false }
/// window: { padding-horizontal: 10, padding-vertical: 6, opacity: 1 }
/// contrast: standard
/// rail-icon-size: 14         # glyphs in the icon rail, in points
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct AppearanceConfig {
    /// File format version.
    pub schema: i64,
    /// Theme ids per appearance.
    pub theme: ThemeChoice,
    /// Global font settings.
    pub font: FontSettings,
    /// Global cursor settings.
    pub cursor: CursorSettings,
    /// Global window settings.
    pub window: WindowSettings,
    /// Global contrast level.
    pub contrast: ContrastLevel,
    /// Size of the icon rail's glyphs, in points; `None` is [`Self::DEFAULT_RAIL_ICON_SIZE`]. App chrome, so
    /// global only: a theme can't set it. YAML key `rail-icon-size`.
    pub rail_icon_size: Option<f64>,
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        Self {
            schema: TerminalTheme::CURRENT_SCHEMA,
            theme: ThemeChoice::default(),
            font: FontSettings::default(),
            cursor: CursorSettings::default(),
            window: WindowSettings::default(),
            contrast: ContrastLevel::Standard,
            rail_icon_size: None,
        }
    }
}

impl AppearanceConfig {
    /// Rail glyph size when `appearance.yaml` doesn't set one, in points.
    pub const DEFAULT_RAIL_ICON_SIZE: f64 = 14.0;
    /// Allowed rail glyph sizes, in points.
    pub const RAIL_ICON_SIZE_RANGE: RangeInclusive<f64> = 10.0..=28.0;

    /// The rail's glyph size in points: the configured one, or the default.
    pub fn rail_icon_size(&self) -> f64 {
        self.rail_icon_size.unwrap_or(Self::DEFAULT_RAIL_ICON_SIZE)
    }

    /// Checks the schema and every value's range.
    ///
    /// # Errors
    /// The first problem found, as a [`ThemeError`].
    pub fn validated(&self) -> Result<AppearanceConfig, ThemeError> {
        if !(1..=TerminalTheme::CURRENT_SCHEMA).contains(&self.schema) {
            return Err(ThemeError::UnsupportedSchema(self.schema));
        }
        self.font.validate()?;
        self.window.validate()?;
        if let Some(size) = self.rail_icon_size
            && !Self::RAIL_ICON_SIZE_RANGE.contains(&size)
        {
            return Err(ThemeError::OutOfRange("rail icon size".into(), size));
        }
        Ok(self.clone())
    }

    /// Parses `appearance.yaml` text (not validated; see [`AppearanceConfig::validated`]).
    ///
    /// # Errors
    /// [`ThemeError::Unreadable`] when the text isn't YAML of the right shape, or the error a value raised.
    pub fn from_yaml(text: &str) -> Result<AppearanceConfig, ThemeError> {
        crate::yaml::decode_config(text).map_err(|error| error.into_theme_error(""))
    }

    /// The YAML body `ThemeStore::save_config` writes (without its header comment).
    pub fn to_yaml(&self) -> String {
        crate::yaml::encode_config(self)
    }
}

/// Everything the terminal needs, after a theme's overrides are laid over `AppearanceConfig` and both over the
/// built-in defaults.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedAppearance {
    /// The theme in use.
    pub theme: TerminalTheme,
    /// Font family.
    pub font_family: String,
    /// Font size in points.
    pub font_size: f64,
    /// Draw bold text in the bright ANSI colours.
    pub bold_is_bright: bool,
    /// Cursor shape.
    pub cursor_shape: CursorShape,
    /// Whether the cursor blinks.
    pub cursor_blink: bool,
    /// Left padding in points.
    pub padding_horizontal: f64,
    /// Top and bottom padding in points.
    pub padding_vertical: f64,
    /// Background opacity, 1 is opaque.
    pub opacity: f64,
    /// Contrast level.
    pub contrast: ContrastLevel,
}

impl ResolvedAppearance {
    /// Font family when neither the theme nor `appearance.yaml` sets one.
    pub const DEFAULT_FONT_FAMILY: &'static str = "SF Mono";
    /// Font size when neither the theme nor `appearance.yaml` sets one.
    pub const DEFAULT_FONT_SIZE: f64 = 13.0;

    /// The theme's own settings win, then `config`'s, then the built-in defaults.
    pub fn new(theme: TerminalTheme, config: &AppearanceConfig) -> Self {
        let font = theme.font.clone().unwrap_or_default();
        let cursor = theme.cursor.unwrap_or_default();
        let window = theme.window.unwrap_or_default();
        Self {
            font_family: font
                .family
                .or_else(|| config.font.family.clone())
                .unwrap_or_else(|| Self::DEFAULT_FONT_FAMILY.to_owned()),
            font_size: font
                .size
                .or(config.font.size)
                .unwrap_or(Self::DEFAULT_FONT_SIZE),
            bold_is_bright: font
                .bold_is_bright
                .or(config.font.bold_is_bright)
                .unwrap_or(true),
            cursor_shape: cursor
                .style
                .or(config.cursor.style)
                .unwrap_or(CursorShape::Block),
            cursor_blink: cursor.blink.or(config.cursor.blink).unwrap_or(false),
            padding_horizontal: window
                .padding_horizontal
                .or(config.window.padding_horizontal)
                .unwrap_or(10.0),
            padding_vertical: window
                .padding_vertical
                .or(config.window.padding_vertical)
                .unwrap_or(6.0),
            opacity: window.opacity.or(config.window.opacity).unwrap_or(1.0),
            contrast: theme.contrast.unwrap_or(config.contrast),
            theme,
        }
    }
}
