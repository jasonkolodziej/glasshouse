//! Turns other terminals' theme files into [`TerminalTheme`]s.

use std::collections::HashMap;
use std::io::Cursor;

use plist::{Dictionary, Value};

use crate::{AnsiColors, Palette, TerminalTheme, ThemeColor, ThemeError};

/// Readers for iTerm2 and Ghostty themes.
#[derive(Debug, Clone, Copy)]
pub struct ThemeImporter;

impl ThemeImporter {
    /// iTerm2's `.itermcolors`: a property list (XML or binary) of colour dictionaries ("Ansi 0 Color" …
    /// "Ansi 15 Color", "Background Color", "Foreground Color", …) with 0–1 components. Components are taken as
    /// sRGB; iTerm2's "Calibrated" and P3 colours come out slightly different.
    ///
    /// # Errors
    /// [`ThemeError::UnknownFormat`] with `name` when the data isn't such a property list or lacks the background or
    /// foreground colour.
    pub fn iterm(data: &[u8], name: &str) -> Result<TerminalTheme, ThemeError> {
        let unknown = || ThemeError::UnknownFormat(name.to_owned());
        let root = Value::from_reader(Cursor::new(data)).map_err(|_| unknown())?;
        let root = root.as_dictionary().ok_or_else(unknown)?;
        let color = |key: &str| iterm_color(root, key);
        let (Some(background), Some(foreground)) =
            (color("Background Color"), color("Foreground Color"))
        else {
            return Err(unknown());
        };
        let ansi: Vec<Option<ThemeColor>> =
            (0..16).map(|i| color(&format!("Ansi {i} Color"))).collect();
        Ok(TerminalTheme::new(
            name,
            Self::palette(
                background,
                foreground,
                color("Bold Color"),
                color("Cursor Color"),
                color("Cursor Text Color"),
                color("Selection Color"),
                &ansi,
            ),
        ))
    }

    /// Ghostty's theme format: `key = value` lines, such as `background = #282a36` and `palette = 1=#ff5555`.
    /// Unknown keys and values that aren't colours are skipped.
    ///
    /// # Errors
    /// [`ThemeError::UnknownFormat`] with `name` when there is no background or foreground colour.
    pub fn ghostty(text: &str, name: &str) -> Result<TerminalTheme, ThemeError> {
        let mut values: HashMap<&str, ThemeColor> = HashMap::new();
        let mut ansi: Vec<Option<ThemeColor>> = vec![None; 16];
        let is_space = |c: char| c.is_whitespace() && !crate::terminal_theme::is_newline(c);
        for raw in text
            .split(crate::terminal_theme::is_newline)
            .filter(|line| !line.is_empty())
        {
            let line = raw.trim_matches(is_space);
            if line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let (key, value) = (key.trim_matches(is_space), value.trim_matches(is_space));
            match (key, value.split_once('=')) {
                ("palette", Some((index, color))) => {
                    if let Ok(index) = index.trim_matches(is_space).parse::<usize>()
                        && let Some(slot) = ansi.get_mut(index)
                    {
                        *slot = ThemeColor::from_hex(color).ok();
                    }
                }
                _ => {
                    if let Ok(color) = ThemeColor::from_hex(value) {
                        values.insert(key, color);
                    }
                }
            }
        }
        let (Some(&background), Some(&foreground)) =
            (values.get("background"), values.get("foreground"))
        else {
            return Err(ThemeError::UnknownFormat(name.to_owned()));
        };
        Ok(TerminalTheme::new(
            name,
            Self::palette(
                background,
                foreground,
                values.get("bold-color").copied(),
                values.get("cursor-color").copied(),
                values.get("cursor-text").copied(),
                values.get("selection-background").copied(),
                &ansi,
            ),
        ))
    }

    /// Missing ANSI colours come from Glasshouse's palette (normal row) or are derived from the normal row (bright).
    /// `ansi` holds up to 16 colours, normal then bright; missing entries count as `None`.
    pub fn palette(
        background: ThemeColor,
        foreground: ThemeColor,
        bold: Option<ThemeColor>,
        cursor: Option<ThemeColor>,
        cursor_text: Option<ThemeColor>,
        selection: Option<ThemeColor>,
        ansi: &[Option<ThemeColor>],
    ) -> Palette {
        let at = |i: usize| ansi.get(i).copied().flatten();
        let defaults = TerminalTheme::GLASSHOUSE_NORMAL.all();
        let normal: [ThemeColor; 8] = std::array::from_fn(|i| at(i).unwrap_or(defaults[i]));
        let toward = if background.is_dark() {
            ThemeColor::WHITE
        } else {
            ThemeColor::BLACK
        };
        let bright: [ThemeColor; 8] =
            std::array::from_fn(|i| at(i + 8).unwrap_or_else(|| normal[i].mixed(&toward, 0.25)));
        Palette {
            background,
            foreground,
            bold,
            cursor,
            cursor_text,
            selection,
            normal: AnsiColors::from_array(normal),
            bright: AnsiColors::from_array(bright),
        }
    }
}

fn iterm_color(root: &Dictionary, key: &str) -> Option<ThemeColor> {
    let dict = root.get(key)?.as_dictionary()?;
    let component = |name: &str| match dict.get(name)? {
        Value::Real(value) => Some(*value),
        Value::Integer(value) => value
            .as_signed()
            .map(|v| v as f64)
            .or_else(|| value.as_unsigned().map(|v| v as f64)),
        Value::Boolean(flag) => Some(if *flag { 1.0 } else { 0.0 }),
        _ => None,
    };
    Some(ThemeColor::with_alpha(
        component("Red Component")?,
        component("Green Component")?,
        component("Blue Component")?,
        component("Alpha Component").unwrap_or(1.0),
    ))
}
