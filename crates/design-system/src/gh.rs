//! Design tokens (`GH` in Swift). One source of truth for every custom glass surface.
//!
//! Lengths are in points (logical pixels).

/// Corner radii.
pub mod radius {
    /// Cards (approval card, popovers).
    pub const CARD: f32 = 18.0;
    /// Command palette.
    pub const PALETTE: f32 = 22.0;
    /// Fully rounded capsule ends.
    pub const CAPSULE: f32 = 999.0;
    /// Bars.
    pub const BAR: f32 = 14.0;
}

/// Spacing scale.
pub mod space {
    /// Extra small.
    pub const XS: f32 = 4.0;
    /// Small.
    pub const S: f32 = 8.0;
    /// Medium.
    pub const M: f32 = 12.0;
    /// Large.
    pub const L: f32 = 16.0;
    /// Extra large.
    pub const XL: f32 = 24.0;
}

/// Fixed component sizes.
pub mod size {
    /// Reserved trailing inset for the command-mark gutter — outside the cell grid, never over text.
    pub const GUTTER_WIDTH: f32 = 12.0;
    /// Length of a command-mark tick in the gutter.
    pub const GUTTER_TICK: f32 = 6.0;
    /// Hit-target height of a gutter mark.
    pub const GUTTER_HIT_HEIGHT: f32 = 16.0;
    /// Card width.
    pub const CARD_WIDTH: f32 = 400.0;
    /// Command palette width.
    pub const PALETTE_WIDTH: f32 = 600.0;
    /// Sidebar width.
    pub const SIDEBAR_WIDTH: f32 = 248.0;
    /// Tab bubble height. With `TAB_GROUP_INSET` on each side the group is 36 pt, the height of
    /// the toolbar's own glass buttons, and the tab and group capsules are concentric.
    pub const TAB_HEIGHT: f32 = 28.0;
    /// Inset between the tab group capsule and its tabs.
    pub const TAB_GROUP_INSET: f32 = 4.0;
    /// Gap between tabs.
    pub const TAB_SPACING: f32 = 2.0;
}

/// A non-bouncy spring (SwiftUI `.smooth(duration:)`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Motion {
    /// Perceptual duration in seconds.
    pub duration: f32,
    /// Spring bounce; 0 = critically damped.
    pub bounce: f32,
}

/// One spring for every L3 transient. Under Reduce Motion, transitions become opacity-only
/// (`ghTransition`), and state changes should apply without animation.
pub const MOTION: Motion = Motion {
    duration: 0.25,
    bounce: 0.0,
};

/// Opacity of the black scrim drawn over content behind a transient (palette, approval card).
pub const DIM_OPACITY: f32 = 0.28;

/// Opaque fallback for glass under Reduce Transparency or Increase Contrast: Glasshouse-owned
/// surfaces fill with the window background color and get a border.
pub mod glass_fallback {
    /// Border width under Increase Contrast (drawn in the primary label color).
    pub const BORDER_WIDTH_INCREASED_CONTRAST: f32 = 1.5;
    /// Border width under Reduce Transparency only (secondary label color at `BORDER_OPACITY`).
    pub const BORDER_WIDTH: f32 = 0.5;
    /// Opacity of the secondary-color border under Reduce Transparency only.
    pub const BORDER_OPACITY: f32 = 0.3;
}

/// Layer roles from the design spec. Only `Controls` and `Transient` may be glass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChromeLayer {
    /// Toolbars, tab bars, sidebars.
    #[default]
    Controls,
    /// Palettes, cards and other short-lived overlays.
    Transient,
}

/// Edge a transient slides in from (opacity only under Reduce Motion).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionEdge {
    /// Top edge.
    Top,
    /// Bottom edge.
    Bottom,
    /// Leading edge.
    Leading,
    /// Trailing edge.
    Trailing,
}
