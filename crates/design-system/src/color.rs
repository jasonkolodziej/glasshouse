/// A plain sRGB color with straight alpha; every channel is in `0.0..=1.0`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rgba {
    /// Red, 0…1.
    pub r: f32,
    /// Green, 0…1.
    pub g: f32,
    /// Blue, 0…1.
    pub b: f32,
    /// Alpha, 0…1 (1 = opaque).
    pub a: f32,
}

impl Rgba {
    /// Opaque black.
    pub const BLACK: Self = Self::rgb(0.0, 0.0, 0.0);
    /// Opaque white.
    pub const WHITE: Self = Self::rgb(1.0, 1.0, 1.0);

    /// A color from components, each 0…1.
    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// An opaque color from components, each 0…1.
    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b, a: 1.0 }
    }

    /// An opaque color from 8-bit channels.
    pub fn from_rgb8(r: u8, g: u8, b: u8) -> Self {
        Self::rgb(
            f32::from(r) / 255.0,
            f32::from(g) / 255.0,
            f32::from(b) / 255.0,
        )
    }

    /// An opaque color from `0xRRGGBB`.
    pub fn from_hex(hex: u32) -> Self {
        let [_, r, g, b] = hex.to_be_bytes();
        Self::from_rgb8(r, g, b)
    }

    /// The same color with a different alpha.
    #[must_use]
    pub const fn with_alpha(self, a: f32) -> Self {
        Self { a, ..self }
    }

    /// Channels rounded to 8 bits (alpha ignored).
    pub fn to_rgb8(self) -> [u8; 3] {
        let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        [q(self.r), q(self.g), q(self.b)]
    }

    /// Linear interpolation of the RGB channels toward `target` by `t` (0…1); alpha is kept.
    #[must_use]
    pub fn mix(self, target: Self, t: f32) -> Self {
        Self {
            r: self.r + (target.r - self.r) * t,
            g: self.g + (target.g - self.g) * t,
            b: self.b + (target.b - self.b) * t,
            a: self.a,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips_through_rgb8() {
        assert_eq!(Rgba::from_hex(0x16181D).to_rgb8(), [0x16, 0x18, 0x1D]);
    }
}
