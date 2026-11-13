//! The theme model: colours plus optional font, cursor, window and contrast overrides.

use std::ops::RangeInclusive;

use thiserror::Error;

use crate::ThemeColor;

/// Formats a number the way Swift's `Double.description` does (`100.0`, `0.9`, `1e+16`), so messages match the
/// Swift app's.
pub(crate) fn swift_number(value: f64) -> String {
    if value.is_nan() {
        return "nan".into();
    }
    if value.is_infinite() {
        return if value > 0.0 {
            "inf".into()
        } else {
            "-inf".into()
        };
    }
    let magnitude = value.abs();
    if magnitude != 0.0 && !(1e-4..1e16).contains(&magnitude) {
        let text = format!("{value:e}");
        return match text.split_once('e') {
            Some((mantissa, exponent)) => {
                let (sign, digits) = exponent
                    .strip_prefix('-')
                    .map_or(("+", exponent), |rest| ("-", rest));
                format!("{mantissa}e{sign}{digits:0>2}")
            }
            None => text,
        };
    }
    if value.fract() == 0.0 {
        format!("{value:.1}")
    } else {
        format!("{value}")
    }
}

/// Swift's `Character.isNewline`.
pub(crate) fn is_newline(c: char) -> bool {
    matches!(
        c,
        '\n' | '\r' | '\u{0B}' | '\u{0C}' | '\u{85}' | '\u{2028}' | '\u{2029}'
    )
}

/// Everything that can go wrong reading, validating or storing a theme. `Display` gives a message for people.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum ThemeError {
    /// A colour that isn't `#rgb`, `#rrggbb` or `#rrggbbaa`.
    #[error("“{0}” isn't a colour. Use #rrggbb, for example #1e1f22.")]
    InvalidColor(String),
    /// An empty, overlong or multi-line theme name.
    #[error("“{0}” can't be used as a theme name. Use 1–80 characters on one line.")]
    InvalidName(String),
    /// An author or source that is too long or multi-line (holds the first 40 characters).
    #[error("“{0}…” is too long or spans several lines.")]
    InvalidText(String),
    /// An empty, overlong or multi-line font family.
    #[error("“{0}” isn't a font family name.")]
    InvalidFont(String),
    /// A number outside its allowed range: what it is, and the value.
    #[error("{value} is out of range for the {what}.", value = swift_number(*.1), what = .0)]
    OutOfRange(String, f64),
    /// A `schema` this version can't read.
    #[error("This theme uses format {0}, which this version of Glasshouse can't read.")]
    UnsupportedSchema(i64),
    /// A file that couldn't be read or parsed.
    #[error("{0} couldn't be read as a theme.")]
    Unreadable(String),
    /// A required key is missing; holds its dotted path, such as `colors.background`.
    #[error("The theme has no “{0}”, which every theme needs.")]
    MissingKey(String),
    /// An imported file in no format Glasshouse knows.
    #[error("{0} isn't a Glasshouse, iTerm2 or Ghostty theme.")]
    UnknownFormat(String),
    /// An attempt to change a built-in theme.
    #[error("“{0}” is a built-in theme. Customize a copy instead.")]
    ReadOnly(String),
    /// No theme with this id.
    #[error("There's no theme “{0}”.")]
    NotFound(String),
    /// Writing or deleting a file failed. (Swift surfaced Foundation's own error here.)
    #[error("{path} couldn't be saved: {message}")]
    Io {
        /// The file or folder involved.
        path: String,
        /// The operating system's explanation.
        message: String,
    },
}

/// Which system appearance a theme is made for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Appearance {
    /// Dark Mode.
    Dark,
    /// Light Mode.
    Light,
}

impl Appearance {
    /// Every case, in declaration order.
    pub const ALL: [Appearance; 2] = [Appearance::Dark, Appearance::Light];

    /// The YAML spelling: `dark` or `light`.
    pub fn as_str(self) -> &'static str {
        match self {
            Appearance::Dark => "dark",
            Appearance::Light => "light",
        }
    }

    /// Parses the YAML spelling.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.as_str() == name)
    }
}

/// The eight colours of one ANSI row (normal or bright).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnsiColors {
    /// ANSI 0 / 8.
    pub black: ThemeColor,
    /// ANSI 1 / 9.
    pub red: ThemeColor,
    /// ANSI 2 / 10.
    pub green: ThemeColor,
    /// ANSI 3 / 11.
    pub yellow: ThemeColor,
    /// ANSI 4 / 12.
    pub blue: ThemeColor,
    /// ANSI 5 / 13.
    pub magenta: ThemeColor,
    /// ANSI 6 / 14.
    pub cyan: ThemeColor,
    /// ANSI 7 / 15.
    pub white: ThemeColor,
}

impl AnsiColors {
    /// The YAML key of each colour, in ANSI order.
    pub const NAMES: [&'static str; 8] = [
        "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
    ];

    /// Exactly eight colours, in ANSI order.
    pub const fn from_array(colors: [ThemeColor; 8]) -> Self {
        let [black, red, green, yellow, blue, magenta, cyan, white] = colors;
        Self {
            black,
            red,
            green,
            yellow,
            blue,
            magenta,
            cyan,
            white,
        }
    }

    /// The eight colours, in ANSI order.
    pub fn all(&self) -> [ThemeColor; 8] {
        [
            self.black,
            self.red,
            self.green,
            self.yellow,
            self.blue,
            self.magenta,
            self.cyan,
            self.white,
        ]
    }

    /// Replaces all eight colours.
    pub fn set_all(&mut self, colors: [ThemeColor; 8]) {
        *self = Self::from_array(colors);
    }

    /// Each colour moved `amount` of the way towards `target`: a quick way to make a bright row.
    pub fn mixed(&self, target: &ThemeColor, amount: f64) -> Self {
        Self::from_array(self.all().map(|c| c.mixed(target, amount)))
    }
}

/// A theme's colours.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    /// Terminal background.
    pub background: ThemeColor,
    /// Default text.
    pub foreground: ThemeColor,
    /// Bold text; `None` uses the foreground.
    pub bold: Option<ThemeColor>,
    /// `None` uses the system accent colour.
    pub cursor: Option<ThemeColor>,
    /// Text under a block cursor; `None` lets the terminal choose. YAML key `cursor-text`.
    pub cursor_text: Option<ThemeColor>,
    /// `None` uses the system selection colour.
    pub selection: Option<ThemeColor>,
    /// ANSI 0–7.
    pub normal: AnsiColors,
    /// ANSI 8–15.
    pub bright: AnsiColors,
}

impl Palette {
    /// A palette with the optional colours left unset.
    pub fn new(
        background: ThemeColor,
        foreground: ThemeColor,
        normal: AnsiColors,
        bright: AnsiColors,
    ) -> Self {
        Self {
            background,
            foreground,
            bold: None,
            cursor: None,
            cursor_text: None,
            selection: None,
            normal,
            bright,
        }
    }

    /// All 16 ANSI colours, normal then bright.
    pub fn ansi(&self) -> [ThemeColor; 16] {
        let mut colors = [ThemeColor::BLACK; 16];
        colors[..8].copy_from_slice(&self.normal.all());
        colors[8..].copy_from_slice(&self.bright.all());
        colors
    }

    /// The bright row a theme gets when it leaves it out: the normal row a quarter of the way towards white on a
    /// dark background, or black on a light one.
    pub fn derived_bright(normal: &AnsiColors, background: &ThemeColor) -> AnsiColors {
        normal.mixed(
            if background.is_dark() {
                &ThemeColor::WHITE
            } else {
                &ThemeColor::BLACK
            },
            0.25,
        )
    }
}

/// A terminal theme, stored as YAML. Colours are required; `font`, `cursor`, `window` and `contrast` are optional
/// and, when present, override the global settings in `appearance.yaml` while this theme is in use.
#[derive(Debug, Clone, PartialEq)]
pub struct TerminalTheme {
    /// The file name without `.yaml`. Not stored in the file; empty for a theme not saved yet.
    pub id: String,
    /// File format version.
    pub schema: i64,
    /// Display name.
    pub name: String,
    /// Who made the colours.
    pub author: Option<String>,
    /// Where the colours come from: a URL or a credit line.
    pub source: Option<String>,
    /// Which system appearance the theme is made for. Only used to suggest it; any theme fits either slot.
    pub appearance: Appearance,
    /// The colours.
    pub colors: Palette,
    /// Font overrides.
    pub font: Option<FontSettings>,
    /// Cursor overrides.
    pub cursor: Option<CursorSettings>,
    /// Window overrides.
    pub window: Option<WindowSettings>,
    /// Contrast override.
    pub contrast: Option<ContrastLevel>,
}

impl TerminalTheme {
    /// The newest file format this version reads and writes.
    pub const CURRENT_SCHEMA: i64 = 1;

    /// Glasshouse's own palette, tuned to read on both light and dark backgrounds.
    pub const GLASSHOUSE_NORMAL: AnsiColors = AnsiColors::from_array([
        ThemeColor::from_rgb8(0x1e, 0x1f, 0x22),
        ThemeColor::from_rgb8(0xd1, 0x3b, 0x3b),
        ThemeColor::from_rgb8(0x2e, 0x9e, 0x5b),
        ThemeColor::from_rgb8(0xc9, 0x8a, 0x10),
        ThemeColor::from_rgb8(0x2f, 0x6f, 0xd6),
        ThemeColor::from_rgb8(0xa2, 0x4b, 0xc9),
        ThemeColor::from_rgb8(0x1e, 0x96, 0xa8),
        ThemeColor::from_rgb8(0xc8, 0xc8, 0xc4),
    ]);

    /// The bright row of Glasshouse's own palette.
    pub const GLASSHOUSE_BRIGHT: AnsiColors = AnsiColors::from_array([
        ThemeColor::from_rgb8(0x5c, 0x5f, 0x66),
        ThemeColor::from_rgb8(0xf0, 0x5a, 0x5a),
        ThemeColor::from_rgb8(0x4c, 0xc3, 0x7a),
        ThemeColor::from_rgb8(0xe8, 0xb3, 0x3b),
        ThemeColor::from_rgb8(0x5b, 0x92, 0xf0),
        ThemeColor::from_rgb8(0xc3, 0x77, 0xe8),
        ThemeColor::from_rgb8(0x3c, 0xc2, 0xd6),
        ThemeColor::from_rgb8(0xff, 0xff, 0xff),
    ]);

    /// A theme with no id, the current schema, an appearance derived from the background and no overrides.
    pub fn new(name: impl Into<String>, colors: Palette) -> Self {
        Self {
            id: String::new(),
            schema: Self::CURRENT_SCHEMA,
            name: name.into(),
            author: None,
            source: None,
            appearance: if colors.background.is_dark() {
                Appearance::Dark
            } else {
                Appearance::Light
            },
            colors,
            font: None,
            cursor: None,
            window: None,
            contrast: None,
        }
    }

    /// Checks everything a hand-edited file could get wrong, and returns the theme with its name trimmed.
    ///
    /// # Errors
    /// The first problem found, as a [`ThemeError`].
    pub fn validated(&self) -> Result<TerminalTheme, ThemeError> {
        if !(1..=Self::CURRENT_SCHEMA).contains(&self.schema) {
            return Err(ThemeError::UnsupportedSchema(self.schema));
        }
        let trimmed_name = self.name.trim();
        if trimmed_name.is_empty()
            || trimmed_name.chars().count() > 80
            || trimmed_name.chars().any(is_newline)
        {
            return Err(ThemeError::InvalidName(self.name.clone()));
        }
        for text in [&self.author, &self.source].into_iter().flatten() {
            if text.chars().count() > 200 || text.chars().any(is_newline) {
                return Err(ThemeError::InvalidText(text.chars().take(40).collect()));
            }
        }
        if let Some(font) = &self.font {
            font.validate()?;
        }
        if let Some(window) = &self.window {
            window.validate()?;
        }
        let mut theme = self.clone();
        theme.name = trimmed_name.to_owned();
        Ok(theme)
    }

    /// A theme with the same colours that is yours to edit: no id, a new name, no author or source.
    pub fn copy_named(&self, new_name: impl Into<String>) -> TerminalTheme {
        TerminalTheme {
            id: String::new(),
            name: new_name.into(),
            author: None,
            source: None,
            ..self.clone()
        }
    }
}

/// Font settings. Every field is optional so a theme can override just one; `AppearanceConfig` fills the rest.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FontSettings {
    /// Font family name.
    pub family: Option<String>,
    /// Point size, within [`FontSettings::SIZE_RANGE`].
    pub size: Option<f64>,
    /// Draw bold text in the bright ANSI colours, as most terminals do. YAML key `bold-is-bright`.
    pub bold_is_bright: Option<bool>,
}

impl FontSettings {
    /// Allowed font sizes, in points.
    pub const SIZE_RANGE: RangeInclusive<f64> = 6.0..=72.0;

    pub(crate) fn validate(&self) -> Result<(), ThemeError> {
        if let Some(family) = &self.family
            && (family.trim().is_empty()
                || family.chars().count() > 100
                || family.chars().any(is_newline))
        {
            return Err(ThemeError::InvalidFont(family.clone()));
        }
        if let Some(size) = self.size
            && !Self::SIZE_RANGE.contains(&size)
        {
            return Err(ThemeError::OutOfRange("font size".into(), size));
        }
        Ok(())
    }
}

/// The cursor's shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CursorShape {
    /// A filled cell.
    Block,
    /// A line under the cell.
    Underline,
    /// A vertical bar before the cell.
    Bar,
}

impl CursorShape {
    /// Every case, in declaration order.
    pub const ALL: [CursorShape; 3] =
        [CursorShape::Block, CursorShape::Underline, CursorShape::Bar];

    /// The YAML spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            CursorShape::Block => "block",
            CursorShape::Underline => "underline",
            CursorShape::Bar => "bar",
        }
    }

    /// Parses the YAML spelling.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.as_str() == name)
    }
}

/// Cursor settings; every field optional.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CursorSettings {
    /// Shape.
    pub style: Option<CursorShape>,
    /// Whether it blinks.
    pub blink: Option<bool>,
}

/// Window settings; every field optional.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct WindowSettings {
    /// Points between the window edge and the text, left side. YAML key `padding-horizontal`.
    pub padding_horizontal: Option<f64>,
    /// Points above and below the text. YAML key `padding-vertical`.
    pub padding_vertical: Option<f64>,
    /// 1 is opaque. Below 1 the terminal background lets the window material show through.
    pub opacity: Option<f64>,
}

impl WindowSettings {
    /// Allowed padding, in points.
    pub const PADDING_RANGE: RangeInclusive<f64> = 0.0..=64.0;
    /// Allowed opacity.
    pub const OPACITY_RANGE: RangeInclusive<f64> = 0.3..=1.0;

    pub(crate) fn validate(&self) -> Result<(), ThemeError> {
        for (label, value) in [
            ("horizontal padding", self.padding_horizontal),
            ("vertical padding", self.padding_vertical),
        ] {
            if let Some(value) = value
                && !Self::PADDING_RANGE.contains(&value)
            {
                return Err(ThemeError::OutOfRange(label.into(), value));
            }
        }
        if let Some(opacity) = self.opacity
            && !Self::OPACITY_RANGE.contains(&opacity)
        {
            return Err(ThemeError::OutOfRange("opacity".into(), opacity));
        }
        Ok(())
    }
}

/// How far Glasshouse nudges colours to keep text readable (APCA). macOS's Increase Contrast setting always raises
/// it to `High`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum ContrastLevel {
    /// Default text at least Lc 75, ANSI colours at least Lc 60.
    #[default]
    Standard,
    /// Keeps a theme's colours closer to how they were designed: ANSI colours at least Lc 45.
    Faithful,
    /// Default text at least Lc 90, ANSI colours at least Lc 75.
    High,
}

impl ContrastLevel {
    /// Every case, in declaration order.
    pub const ALL: [ContrastLevel; 3] = [
        ContrastLevel::Standard,
        ContrastLevel::Faithful,
        ContrastLevel::High,
    ];

    /// The YAML spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            ContrastLevel::Standard => "standard",
            ContrastLevel::Faithful => "faithful",
            ContrastLevel::High => "high",
        }
    }

    /// Parses the YAML spelling.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.as_str() == name)
    }
}
