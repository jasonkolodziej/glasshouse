//! sRGB colours as written in theme files.

use std::fmt;
use std::str::FromStr;

use crate::ThemeError;

/// An sRGB colour, written in theme files as `"#rrggbb"` (or `"#rrggbbaa"` when not opaque).
///
/// Components are always within 0…1: constructors clamp, and non-finite values become 0.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThemeColor {
    /// Red, 0…1.
    pub red: f64,
    /// Green, 0…1.
    pub green: f64,
    /// Blue, 0…1.
    pub blue: f64,
    /// Alpha, 0…1 (1 is opaque).
    pub alpha: f64,
}

fn clamp_component(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// Foundation's `.whitespaces`: whitespace that isn't a line break.
fn is_inline_whitespace(c: char) -> bool {
    c.is_whitespace() && !crate::terminal_theme::is_newline(c)
}

impl ThemeColor {
    /// Opaque black.
    pub const BLACK: ThemeColor = ThemeColor::from_rgb8(0, 0, 0);
    /// Opaque white.
    pub const WHITE: ThemeColor = ThemeColor::from_rgb8(255, 255, 255);

    /// An opaque colour from 0…1 components (clamped).
    pub fn new(red: f64, green: f64, blue: f64) -> Self {
        Self::with_alpha(red, green, blue, 1.0)
    }

    /// A colour from 0…1 components including alpha (clamped).
    pub fn with_alpha(red: f64, green: f64, blue: f64, alpha: f64) -> Self {
        Self {
            red: clamp_component(red),
            green: clamp_component(green),
            blue: clamp_component(blue),
            alpha: clamp_component(alpha),
        }
    }

    /// An opaque colour from 8-bit components; identical to parsing the matching `#rrggbb`.
    pub const fn from_rgb8(red: u8, green: u8, blue: u8) -> Self {
        Self {
            red: red as f64 / 255.0,
            green: green as f64 / 255.0,
            blue: blue as f64 / 255.0,
            alpha: 1.0,
        }
    }

    /// Parses `#rgb`, `#rrggbb` or `#rrggbbaa`, with or without the `#`, ignoring surrounding spaces.
    ///
    /// # Errors
    /// [`ThemeError::InvalidColor`] with the original input when it isn't one of those forms.
    pub fn from_hex(hex: &str) -> Result<Self, ThemeError> {
        let invalid = || ThemeError::InvalidColor(hex.to_owned());
        let trimmed = hex.trim_matches(is_inline_whitespace);
        let digits = trimmed.strip_prefix('#').unwrap_or(trimmed);
        if !digits.chars().all(|c| c.is_ascii_hexdigit()) || ![3, 6, 8].contains(&digits.len()) {
            return Err(invalid());
        }
        let expanded: String = if digits.len() == 3 {
            digits.chars().flat_map(|c| [c, c]).collect()
        } else {
            digits.to_owned()
        };
        let value = u64::from_str_radix(&expanded, 16).map_err(|_| invalid())?;
        let byte = |shift: u32| ((value >> shift) & 0xFF) as f64 / 255.0;
        Ok(if expanded.len() == 8 {
            Self::with_alpha(byte(24), byte(16), byte(8), byte(0))
        } else {
            Self::new(byte(16), byte(8), byte(0))
        })
    }

    /// Lowercase `#rrggbb`, or `#rrggbbaa` when alpha is below 1.
    pub fn hex(&self) -> String {
        let mut parts = vec![self.red, self.green, self.blue];
        if self.alpha < 1.0 {
            parts.push(self.alpha);
        }
        let mut text = String::from("#");
        for part in parts {
            text.push_str(&format!("{:02x}", (part * 255.0).round() as i64));
        }
        text
    }

    /// WCAG 2 relative luminance.
    pub fn relative_luminance(&self) -> f64 {
        fn linear(c: f64) -> f64 {
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * linear(self.red) + 0.7152 * linear(self.green) + 0.0722 * linear(self.blue)
    }

    /// WCAG 2 contrast ratio, 1…21.
    pub fn contrast_ratio(&self, other: &ThemeColor) -> f64 {
        let (a, b) = (self.relative_luminance(), other.relative_luminance());
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    /// Whether the colour reads as dark (relative luminance below 0.18).
    pub fn is_dark(&self) -> bool {
        self.relative_luminance() < 0.18
    }

    /// `amount` of the way towards `other` (0 = self, 1 = other), in sRGB. `amount` is clamped to 0…1.
    pub fn mixed(&self, other: &ThemeColor, amount: f64) -> ThemeColor {
        let t = amount.clamp(0.0, 1.0);
        let lerp = |a: f64, b: f64| a + (b - a) * t;
        ThemeColor::with_alpha(
            lerp(self.red, other.red),
            lerp(self.green, other.green),
            lerp(self.blue, other.blue),
            lerp(self.alpha, other.alpha),
        )
    }
}

impl fmt::Display for ThemeColor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.hex())
    }
}

impl FromStr for ThemeColor {
    type Err = ThemeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_hex(s)
    }
}
