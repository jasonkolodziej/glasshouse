//! Reading and writing the YAML of themes and `appearance.yaml`, with the same keys, order and error reporting as
//! the Swift app's Codable types.

use serde_yaml_ng::{Mapping, Value};

use crate::terminal_theme::swift_number;
use crate::{
    AnsiColors, Appearance, AppearanceConfig, ContrastLevel, CursorSettings, CursorShape,
    FontSettings, Palette, TerminalTheme, ThemeChoice, ThemeColor, ThemeError, WindowSettings,
};

/// Why decoding stopped, before it is turned into a [`ThemeError`].
#[derive(Debug)]
pub(crate) enum DecodeError {
    /// A value raised its own error (an invalid colour, say).
    Theme(ThemeError),
    /// A required key is missing; holds its dotted path.
    MissingKey(String),
    /// Not YAML, or a value of the wrong type.
    Corrupt,
}

impl DecodeError {
    /// `id` names the file in [`ThemeError::Unreadable`]; empty means "The file".
    pub(crate) fn into_theme_error(self, id: &str) -> ThemeError {
        match self {
            DecodeError::Theme(error) => error,
            DecodeError::MissingKey(path) => ThemeError::MissingKey(path),
            DecodeError::Corrupt => ThemeError::Unreadable(if id.is_empty() {
                "The file".into()
            } else {
                id.into()
            }),
        }
    }
}

type Decoded<T> = Result<T, DecodeError>;

/// A mapping and where it sits in the document, for `MissingKey` paths.
struct Keyed<'a> {
    map: &'a Mapping,
    path: String,
}

impl<'a> Keyed<'a> {
    fn root(value: &'a Value) -> Decoded<Self> {
        Self::at(value, String::new())
    }

    fn at(value: &'a Value, path: String) -> Decoded<Self> {
        match value {
            Value::Mapping(map) => Ok(Self { map, path }),
            _ => Err(DecodeError::Corrupt),
        }
    }

    fn key_path(&self, key: &str) -> String {
        if self.path.is_empty() {
            key.to_owned()
        } else {
            format!("{}.{key}", self.path)
        }
    }

    /// `decodeIfPresent`: absent and null both give `None`.
    fn optional<T>(
        &self,
        key: &str,
        decode: impl FnOnce(&'a Value, String) -> Decoded<T>,
    ) -> Decoded<Option<T>> {
        match self.map.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(value) => decode(value, self.key_path(key)).map(Some),
        }
    }

    /// `decode`: absent is a missing key, null a corrupt value.
    fn required<T>(
        &self,
        key: &str,
        decode: impl FnOnce(&'a Value, String) -> Decoded<T>,
    ) -> Decoded<T> {
        match self.map.get(key) {
            None => Err(DecodeError::MissingKey(self.key_path(key))),
            Some(Value::Null) => Err(DecodeError::Corrupt),
            Some(value) => decode(value, self.key_path(key)),
        }
    }
}

/// Any scalar reads as a string, as with Yams.
fn string(value: &Value, _: String) -> Decoded<String> {
    match value {
        Value::String(text) => Ok(text.clone()),
        Value::Number(number) => Ok(number.to_string()),
        Value::Bool(flag) => Ok(flag.to_string()),
        _ => Err(DecodeError::Corrupt),
    }
}

fn double(value: &Value, _: String) -> Decoded<f64> {
    match value {
        Value::Number(number) => number.as_f64().ok_or(DecodeError::Corrupt),
        _ => Err(DecodeError::Corrupt),
    }
}

fn integer(value: &Value, _: String) -> Decoded<i64> {
    match value {
        Value::Number(number) => number.as_i64().ok_or(DecodeError::Corrupt),
        _ => Err(DecodeError::Corrupt),
    }
}

/// YAML 1.1 booleans, as Yams reads them.
fn boolean(value: &Value, _: String) -> Decoded<bool> {
    match value {
        Value::Bool(flag) => Ok(*flag),
        Value::String(text) => match text.as_str() {
            "true" | "True" | "TRUE" | "yes" | "Yes" | "YES" | "on" | "On" | "ON" => Ok(true),
            "false" | "False" | "FALSE" | "no" | "No" | "NO" | "off" | "Off" | "OFF" => Ok(false),
            _ => Err(DecodeError::Corrupt),
        },
        _ => Err(DecodeError::Corrupt),
    }
}

fn named<T>(parse: fn(&str) -> Option<T>) -> impl Fn(&Value, String) -> Decoded<T> {
    move |value, path| parse(&string(value, path)?).ok_or(DecodeError::Corrupt)
}

fn color(value: &Value, path: String) -> Decoded<ThemeColor> {
    ThemeColor::from_hex(&string(value, path)?).map_err(DecodeError::Theme)
}

fn ansi_colors(value: &Value, path: String) -> Decoded<AnsiColors> {
    let keyed = Keyed::at(value, path)?;
    let mut colors = [ThemeColor::BLACK; 8];
    for (slot, name) in colors.iter_mut().zip(AnsiColors::NAMES) {
        *slot = keyed.required(name, color)?;
    }
    Ok(AnsiColors::from_array(colors))
}

/// A hand-written theme may leave out the ANSI rows: a missing normal row comes from Glasshouse's own palette, and
/// a missing bright row is derived from the normal one.
fn palette(value: &Value, path: String) -> Decoded<Palette> {
    let c = Keyed::at(value, path)?;
    let background = c.required("background", color)?;
    let foreground = c.required("foreground", color)?;
    let bold = c.optional("bold", color)?;
    let cursor = c.optional("cursor", color)?;
    let cursor_text = c.optional("cursor-text", color)?;
    let selection = c.optional("selection", color)?;
    let normal = c
        .optional("normal", ansi_colors)?
        .unwrap_or(TerminalTheme::GLASSHOUSE_NORMAL);
    let bright = match c.optional("bright", ansi_colors)? {
        Some(bright) => bright,
        None => Palette::derived_bright(&normal, &background),
    };
    Ok(Palette {
        background,
        foreground,
        bold,
        cursor,
        cursor_text,
        selection,
        normal,
        bright,
    })
}

fn font(value: &Value, path: String) -> Decoded<FontSettings> {
    let c = Keyed::at(value, path)?;
    Ok(FontSettings {
        family: c.optional("family", string)?,
        size: c.optional("size", double)?,
        bold_is_bright: c.optional("bold-is-bright", boolean)?,
    })
}

fn cursor(value: &Value, path: String) -> Decoded<CursorSettings> {
    let c = Keyed::at(value, path)?;
    Ok(CursorSettings {
        style: c.optional("style", named(CursorShape::from_name))?,
        blink: c.optional("blink", boolean)?,
    })
}

fn window(value: &Value, path: String) -> Decoded<WindowSettings> {
    let c = Keyed::at(value, path)?;
    Ok(WindowSettings {
        padding_horizontal: c.optional("padding-horizontal", double)?,
        padding_vertical: c.optional("padding-vertical", double)?,
        opacity: c.optional("opacity", double)?,
    })
}

fn theme_choice(value: &Value, path: String) -> Decoded<ThemeChoice> {
    let c = Keyed::at(value, path)?;
    let defaults = ThemeChoice::default();
    Ok(ThemeChoice {
        dark: c.optional("dark", string)?.unwrap_or(defaults.dark),
        light: c.optional("light", string)?.unwrap_or(defaults.light),
        follow_system: c
            .optional("follow-system", boolean)?
            .unwrap_or(defaults.follow_system),
    })
}

fn parse(text: &str) -> Decoded<Value> {
    serde_yaml_ng::from_str(text).map_err(|_| DecodeError::Corrupt)
}

/// Decodes a theme (not validated, `id` left empty), in the order Swift's decoder reads the keys.
pub(crate) fn decode_theme(text: &str) -> Decoded<TerminalTheme> {
    let document = parse(text)?;
    let c = Keyed::root(&document)?;
    let colors = c.required("colors", palette)?;
    let mut theme = TerminalTheme::new(c.required("name", string)?, colors);
    theme.author = c.optional("author", string)?;
    theme.source = c.optional("source", string)?;
    if let Some(appearance) = c.optional("appearance", named(Appearance::from_name))? {
        theme.appearance = appearance;
    }
    theme.font = c.optional("font", font)?;
    theme.cursor = c.optional("cursor", cursor)?;
    theme.window = c.optional("window", window)?;
    theme.contrast = c.optional("contrast", named(ContrastLevel::from_name))?;
    theme.schema = c
        .optional("schema", integer)?
        .unwrap_or(TerminalTheme::CURRENT_SCHEMA);
    Ok(theme)
}

/// Decodes `appearance.yaml`, where every key is optional.
pub(crate) fn decode_config(text: &str) -> Decoded<AppearanceConfig> {
    let document = parse(text)?;
    let c = Keyed::root(&document)?;
    Ok(AppearanceConfig {
        theme: c.optional("theme", theme_choice)?.unwrap_or_default(),
        font: c.optional("font", font)?.unwrap_or_default(),
        cursor: c.optional("cursor", cursor)?.unwrap_or_default(),
        window: c.optional("window", window)?.unwrap_or_default(),
        contrast: c
            .optional("contrast", named(ContrastLevel::from_name))?
            .unwrap_or_default(),
        rail_icon_size: c.optional("rail-icon-size", double)?,
        schema: c
            .optional("schema", integer)?
            .unwrap_or(TerminalTheme::CURRENT_SCHEMA),
    })
}

// MARK: Writing

/// Numbers as people write them: `14`, not `14.0` or `1.4e+1`.
pub(crate) fn plain_number(number: f64) -> String {
    if number.is_finite() && number == number.round() && number.abs() < 1e15 {
        format!("{}", number as i64)
    } else {
        swift_number(number)
    }
}

/// A string scalar: plain when YAML reads it back as the same string, otherwise double-quoted.
fn scalar(text: &str) -> String {
    if let Ok(emitted) = serde_yaml_ng::to_string(text) {
        let emitted = emitted.strip_suffix('\n').unwrap_or(&emitted);
        if emitted == text {
            return text.to_owned();
        }
    }
    let mut quoted = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            c if c.is_control() || matches!(c, '\u{85}' | '\u{2028}' | '\u{2029}' | '\u{feff}') => {
                quoted.push_str(&format!("\\u{:04x}", u32::from(c)));
            }
            c => quoted.push(c),
        }
    }
    quoted.push('"');
    quoted
}

fn quoted_color(color: &ThemeColor) -> String {
    format!("\"{}\"", color.hex())
}

/// One `key: value` pair, or `key:` followed by a nested section.
enum Entry {
    Value(&'static str, String),
    Section(&'static str, Vec<Entry>),
}

fn write_entries(out: &mut String, entries: &[Entry], indent: usize) {
    for entry in entries {
        out.push_str(&" ".repeat(indent));
        match entry {
            Entry::Value(key, value) => out.push_str(&format!("{key}: {value}\n")),
            Entry::Section(key, children) if children.is_empty() => {
                out.push_str(&format!("{key}: {{}}\n"))
            }
            Entry::Section(key, children) => {
                out.push_str(&format!("{key}:\n"));
                write_entries(out, children, indent + 2);
            }
        }
    }
}

fn render(entries: &[Entry]) -> String {
    let mut out = String::new();
    write_entries(&mut out, entries, 0);
    out
}

fn ansi_entries(colors: &AnsiColors) -> Vec<Entry> {
    AnsiColors::NAMES
        .iter()
        .zip(colors.all())
        .map(|(name, color)| Entry::Value(name, quoted_color(&color)))
        .collect()
}

fn font_entries(font: &FontSettings) -> Vec<Entry> {
    let mut entries = Vec::new();
    if let Some(family) = &font.family {
        entries.push(Entry::Value("family", scalar(family)));
    }
    if let Some(size) = font.size {
        entries.push(Entry::Value("size", plain_number(size)));
    }
    if let Some(bold_is_bright) = font.bold_is_bright {
        entries.push(Entry::Value("bold-is-bright", bold_is_bright.to_string()));
    }
    entries
}

fn cursor_entries(cursor: &CursorSettings) -> Vec<Entry> {
    let mut entries = Vec::new();
    if let Some(style) = cursor.style {
        entries.push(Entry::Value("style", style.as_str().into()));
    }
    if let Some(blink) = cursor.blink {
        entries.push(Entry::Value("blink", blink.to_string()));
    }
    entries
}

fn window_entries(window: &WindowSettings) -> Vec<Entry> {
    [
        ("padding-horizontal", window.padding_horizontal),
        ("padding-vertical", window.padding_vertical),
        ("opacity", window.opacity),
    ]
    .into_iter()
    .filter_map(|(key, value)| value.map(|value| Entry::Value(key, plain_number(value))))
    .collect()
}

/// A theme's YAML body, keys in the order Swift's encoder writes them; `id` is not stored.
pub(crate) fn encode_theme(theme: &TerminalTheme) -> String {
    let mut entries = vec![
        Entry::Value("schema", theme.schema.to_string()),
        Entry::Value("name", scalar(&theme.name)),
    ];
    if let Some(author) = &theme.author {
        entries.push(Entry::Value("author", scalar(author)));
    }
    if let Some(source) = &theme.source {
        entries.push(Entry::Value("source", scalar(source)));
    }
    entries.push(Entry::Value("appearance", theme.appearance.as_str().into()));

    let colors = &theme.colors;
    let mut palette = vec![
        Entry::Value("background", quoted_color(&colors.background)),
        Entry::Value("foreground", quoted_color(&colors.foreground)),
    ];
    for (key, color) in [
        ("bold", colors.bold),
        ("cursor", colors.cursor),
        ("cursor-text", colors.cursor_text),
        ("selection", colors.selection),
    ] {
        if let Some(color) = color {
            palette.push(Entry::Value(key, quoted_color(&color)));
        }
    }
    palette.push(Entry::Section("normal", ansi_entries(&colors.normal)));
    palette.push(Entry::Section("bright", ansi_entries(&colors.bright)));
    entries.push(Entry::Section("colors", palette));

    if let Some(font) = &theme.font {
        entries.push(Entry::Section("font", font_entries(font)));
    }
    if let Some(cursor) = &theme.cursor {
        entries.push(Entry::Section("cursor", cursor_entries(cursor)));
    }
    if let Some(window) = &theme.window {
        entries.push(Entry::Section("window", window_entries(window)));
    }
    if let Some(contrast) = theme.contrast {
        entries.push(Entry::Value("contrast", contrast.as_str().into()));
    }
    render(&entries)
}

/// `appearance.yaml`'s body, keys in the order Swift's encoder writes them.
pub(crate) fn encode_config(config: &AppearanceConfig) -> String {
    let mut entries = vec![
        Entry::Value("schema", config.schema.to_string()),
        Entry::Section(
            "theme",
            vec![
                Entry::Value("dark", scalar(&config.theme.dark)),
                Entry::Value("light", scalar(&config.theme.light)),
                Entry::Value("follow-system", config.theme.follow_system.to_string()),
            ],
        ),
        Entry::Section("font", font_entries(&config.font)),
        Entry::Section("cursor", cursor_entries(&config.cursor)),
        Entry::Section("window", window_entries(&config.window)),
        Entry::Value("contrast", config.contrast.as_str().into()),
    ];
    // Glasshouse's own key, after the ones Swift writes; omitted at the default.
    if let Some(size) = config.rail_icon_size {
        entries.push(Entry::Value("rail-icon-size", plain_number(size)));
    }
    render(&entries)
}
