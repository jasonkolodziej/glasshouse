//! Glasshouse design system: APCA contrast, design tokens and the trust capsule state.
//!
//! Port of the Swift `DesignSystem` module. SwiftUI views don't port; this crate keeps the data,
//! logic and tokens so any UI framework can render them.

pub mod apca;
mod color;
pub mod gh;
mod trust;

pub use apca::ContrastPolicy;
pub use color::Rgba;
pub use trust::{TrustState, TrustTint};
