//! APCA (Accessible Perceptual Contrast Algorithm), SAPC-APCA 0.0.98G-4g constants.

use crate::Rgba;

const MAIN_TRC: f64 = 2.4;
const S_RCO: f64 = 0.2126729;
const S_GCO: f64 = 0.7151522;
const S_BCO: f64 = 0.0721750;
const NORM_BG: f64 = 0.56;
const NORM_TXT: f64 = 0.57;
const REV_TXT: f64 = 0.62;
const REV_BG: f64 = 0.65;
const BLK_THRS: f64 = 0.022;
const BLK_CLMP: f64 = 1.414;
const SCALE_BOW: f64 = 1.14;
const SCALE_WOB: f64 = 1.14;
const LO_BOW_OFFSET: f64 = 0.027;
const LO_WOB_OFFSET: f64 = 0.027;
const DELTA_Y_MIN: f64 = 0.0005;
const LO_CLIP: f64 = 0.1;

/// Screen luminance (Y) of an sRGB color, APCA-style (simple 2.4 power curve). Alpha is ignored.
pub fn luminance(c: Rgba) -> f64 {
    S_RCO * f64::from(c.r).powf(MAIN_TRC)
        + S_GCO * f64::from(c.g).powf(MAIN_TRC)
        + S_BCO * f64::from(c.b).powf(MAIN_TRC)
}

fn soft_clamp(y: f64) -> f64 {
    if y > BLK_THRS {
        y
    } else {
        y + (BLK_THRS - y).powf(BLK_CLMP)
    }
}

/// APCA lightness contrast (Lc) of `text` on `background`.
///
/// Returns Lc in roughly [-108, 106]; the sign encodes polarity (positive = dark text on light).
/// Glasshouse thresholds (design critique): default foreground ≥ Lc 75, ANSI palette ≥ Lc 60,
/// both raised under Increase Contrast (see [`ContrastPolicy`]).
pub fn contrast(text: Rgba, background: Rgba) -> f64 {
    let yt = soft_clamp(luminance(text));
    let yb = soft_clamp(luminance(background));
    if (yb - yt).abs() < DELTA_Y_MIN {
        return 0.0;
    }

    if yb > yt {
        // Dark text on light background.
        let sapc = (yb.powf(NORM_BG) - yt.powf(NORM_TXT)) * SCALE_BOW;
        if sapc < LO_CLIP {
            0.0
        } else {
            (sapc - LO_BOW_OFFSET) * 100.0
        }
    } else {
        // Light text on dark background.
        let sapc = (yb.powf(REV_BG) - yt.powf(REV_TXT)) * SCALE_WOB;
        if sapc > -LO_CLIP {
            0.0
        } else {
            (sapc + LO_WOB_OFFSET) * 100.0
        }
    }
}

/// Nudges `text` toward white (on a dark background) or black (on a light one) until |Lc| ≥ `minimum`.
///
/// Hue is preserved as long as possible by mixing rather than replacing — the same order Zed uses
/// (lightness first, black/white as last resort). The text's alpha is kept.
pub fn enforce(minimum: f64, text: Rgba, background: Rgba) -> Rgba {
    if contrast(text, background).abs() >= minimum {
        return text;
    }
    let dark_background = luminance(background) < 0.18;
    let target = if dark_background {
        Rgba::WHITE
    } else {
        Rgba::BLACK
    }
    .with_alpha(text.a);
    // Integer steps reproduce Swift's `t += 0.05` loop (20 steps) without float drift in the count.
    for step in 1..=20u8 {
        let candidate = text.mix(target, f32::from(step) * 0.05);
        if contrast(candidate, background).abs() >= minimum {
            return candidate;
        }
    }
    target
}

/// Minimum |Lc| values Glasshouse enforces on terminal colors.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContrastPolicy {
    /// Minimum |Lc| for the default foreground.
    pub foreground_minimum: f64,
    /// Minimum |Lc| for the 16 ANSI palette colors.
    pub palette_minimum: f64,
}

impl ContrastPolicy {
    /// The default policy.
    pub const STANDARD: Self = Self {
        foreground_minimum: 75.0,
        palette_minimum: 60.0,
    };
    /// Opt-out for users who want exact theme colors.
    pub const THEME_FAITHFUL: Self = Self {
        foreground_minimum: 75.0,
        palette_minimum: 45.0,
    };
    /// Used under the system's Increase Contrast setting.
    pub const INCREASED: Self = Self {
        foreground_minimum: 90.0,
        palette_minimum: 75.0,
    };
}

impl Default for ContrastPolicy {
    fn default() -> Self {
        Self::STANDARD
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gray(v: u8) -> Rgba {
        Rgba::from_rgb8(v, v, v)
    }

    // Reference values from the APCA documentation / apca-w3 (0.0.98G-4g).
    #[test]
    fn reference_points() {
        assert!((contrast(gray(0), gray(255)) - 106.04).abs() < 0.05);
        assert!((contrast(gray(255), gray(0)) - (-107.88)).abs() < 0.05);
        assert!((contrast(gray(0x88), gray(255)) - 63.06).abs() < 0.05);
        assert!((contrast(gray(255), gray(0x88)) - (-68.54)).abs() < 0.05);
    }

    #[test]
    fn identical_colors_have_no_contrast() {
        assert_eq!(contrast(gray(120), gray(120)), 0.0);
    }

    #[test]
    fn enforce_raises_dim_red_on_dark_background() {
        let bg = Rgba::from_rgb8(0x16, 0x18, 0x1D);
        let dim_red = Rgba::rgb(f32::from(0x80u8) / 255.0, 0.1, 0.1);
        let fixed = enforce(60.0, dim_red, bg);
        assert!(contrast(fixed, bg).abs() >= 60.0);
        assert!(fixed.r > fixed.g, "still reads as red");
    }

    #[test]
    fn enforce_leaves_passing_colors_alone() {
        let c = gray(240);
        assert_eq!(enforce(75.0, c, gray(10)), c);
    }

    #[test]
    fn enforce_darkens_on_light_background() {
        let fixed = enforce(90.0, gray(200), gray(250));
        assert!(contrast(fixed, gray(250)) >= 90.0 || fixed == Rgba::BLACK);
        assert!(fixed.r < 200.0 / 255.0);
    }
}
