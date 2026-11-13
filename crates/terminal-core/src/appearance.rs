use design_system::Rgba;

/// Cursor shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TerminalCursorShape {
    /// Full cell.
    #[default]
    Block,
    /// Line under the cell.
    Underline,
    /// Vertical bar at the cell's leading edge.
    Bar,
}

/// Visual settings the engine controls. The grid is always drawn opaque unless the user
/// opts into translucency AND Reduce Transparency is off.
///
/// Glasshouse applies its own APCA floor (`design_system::apca::enforce`) to the palette before
/// calling [`TerminalEngine::apply`](crate::TerminalEngine::apply); the engine never adjusts colors.
#[derive(Debug, Clone, PartialEq)]
pub struct TerminalAppearance {
    /// Font family name; the UI falls back to the system monospaced font if it's missing.
    pub font_name: String,
    /// Font size in points.
    pub font_size: f32,
    /// Default text color.
    pub foreground: Rgba,
    /// Default background color.
    pub background: Rgba,
    /// Cursor color.
    pub cursor: Rgba,
    /// Text under a block cursor; `None` lets the UI choose (usually the background).
    pub cursor_text: Option<Rgba>,
    /// Selection highlight; `None` uses the system's.
    pub selection: Option<Rgba>,
    /// Default cursor shape (programs may change it with DECSCUSR).
    pub cursor_shape: TerminalCursorShape,
    /// Whether the default cursor blinks.
    pub cursor_blink: bool,
    /// Bold text in the bright ANSI colours.
    pub bold_is_bright: bool,
    /// Opacity of the default background, 0…1.
    pub background_opacity: f32,
    /// 16 ANSI colors, normal then bright.
    pub ansi: [Rgba; 16],
    /// Space around the grid, drawn by the pane (not the engine) in the background colour.
    pub padding_horizontal: f32,
    /// Vertical counterpart of `padding_horizontal`.
    pub padding_vertical: f32,
}

impl TerminalAppearance {
    /// Swift's initializer defaults ("SF Mono" 13, block cursor, bold-is-bright, opaque, padding 10/6).
    pub fn new(foreground: Rgba, background: Rgba, cursor: Rgba, ansi: [Rgba; 16]) -> Self {
        Self {
            font_name: "SF Mono".into(),
            font_size: 13.0,
            foreground,
            background,
            cursor,
            cursor_text: None,
            selection: None,
            cursor_shape: TerminalCursorShape::Block,
            cursor_blink: false,
            bold_is_bright: true,
            background_opacity: 1.0,
            ansi,
            padding_horizontal: 10.0,
            padding_vertical: 6.0,
        }
    }
}

impl Default for TerminalAppearance {
    /// A neutral dark theme with the xterm palette; real themes come from the theme crate.
    fn default() -> Self {
        const ANSI: [u32; 16] = [
            0x000000, 0xCD0000, 0x00CD00, 0xCDCD00, 0x0000EE, 0xCD00CD, 0x00CDCD, 0xE5E5E5, //
            0x7F7F7F, 0xFF0000, 0x00FF00, 0xFFFF00, 0x5C5CFF, 0xFF00FF, 0x00FFFF, 0xFFFFFF,
        ];
        Self::new(
            Rgba::from_hex(0xE6E6E6),
            Rgba::from_hex(0x16181D),
            Rgba::from_hex(0xE6E6E6),
            ANSI.map(Rgba::from_hex),
        )
    }
}

/// Color of xterm palette index 16…255 (6×6×6 cube, then a 24-step gray ramp).
pub(crate) fn xterm_256(index: u8) -> Rgba {
    if index >= 232 {
        let v = 8 + 10 * (index - 232);
        return Rgba::from_rgb8(v, v, v);
    }
    let i = index.saturating_sub(16);
    let level = |v: u8| if v == 0 { 0 } else { 55 + 40 * v };
    Rgba::from_rgb8(level(i / 36), level((i / 6) % 6), level(i % 6))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xterm_cube_and_gray_ramp() {
        assert_eq!(xterm_256(16).to_rgb8(), [0, 0, 0]);
        assert_eq!(xterm_256(196).to_rgb8(), [255, 0, 0]);
        assert_eq!(xterm_256(110).to_rgb8(), [135, 175, 215]);
        assert_eq!(xterm_256(232).to_rgb8(), [8, 8, 8]);
        assert_eq!(xterm_256(255).to_rgb8(), [238, 238, 238]);
    }
}
