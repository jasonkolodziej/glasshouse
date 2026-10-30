import SwiftUI

/// Design tokens. One source of truth for every custom glass surface (critique: consistency).
public enum GH {
    public enum Radius {
        public static let card: CGFloat = 18
        public static let palette: CGFloat = 22
        public static let capsule: CGFloat = 999
        public static let bar: CGFloat = 14
    }
    public enum Space {
        public static let xs: CGFloat = 4, s: CGFloat = 8, m: CGFloat = 12, l: CGFloat = 16, xl: CGFloat = 24
    }
    public enum Size {
        /// Reserved trailing inset for the command-mark gutter — outside the cell grid, never over text.
        public static let gutterWidth: CGFloat = 12
        public static let gutterTick: CGFloat = 6
        public static let gutterHitHeight: CGFloat = 16
        public static let cardWidth: CGFloat = 400
        public static let paletteWidth: CGFloat = 600
        public static let sidebarWidth: CGFloat = 248
        /// Tab bubble height. With `tabGroupInset` on each side the group is 36 pt, the height of
        /// the toolbar's own glass buttons, and the tab and group capsules are concentric.
        public static let tabHeight: CGFloat = 28
        public static let tabGroupInset: CGFloat = 4
        public static let tabSpacing: CGFloat = 2
        /// Space between the window edge and the terminal grid. The trailing side has the gutter instead.
        public static let terminalPaddingLeading: CGFloat = 10
        public static let terminalPaddingVertical: CGFloat = 6
    }
    /// One spring for every L3 transient; `.identity` under Reduce Motion is applied by `ghTransition`.
    public static let motion: Animation = .smooth(duration: 0.25)
}

/// Layer roles from the design spec. Only `.controls` and `.transient` may be glass.
public enum ChromeLayer { case controls, transient }

private struct GlassChromeModifier: ViewModifier {
    let layer: ChromeLayer
    let cornerRadius: CGFloat
    let tint: Color?
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency
    @Environment(\.colorSchemeContrast) private var contrast

    func body(content: Content) -> some View {
        let shape = RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
        if reduceTransparency || contrast == .increased {
            // Glasshouse-owned surfaces fall back to opaque; system containers adapt on their own.
            content
                .background(shape.fill(Color(nsColor: .windowBackgroundColor)))
                .overlay(shape.strokeBorder(contrast == .increased ? Color.primary : Color.secondary.opacity(0.3),
                                            lineWidth: contrast == .increased ? 1.5 : 0.5))
        } else {
            // Always `.regular`: `.clear` is for media-rich backgrounds with bold foregrounds,
            // which terminal text is not.
            content.glassEffect(tint.map { Glass.regular.tint($0) } ?? .regular, in: shape)
        }
    }
}

public extension View {
    /// Applies Liquid Glass (or its accessible opaque fallback) to a controls/transient surface.
    func glassChrome(_ layer: ChromeLayer = .controls, cornerRadius: CGFloat = GH.Radius.card,
                     tint: Color? = nil) -> some View {
        modifier(GlassChromeModifier(layer: layer, cornerRadius: cornerRadius, tint: tint))
    }

    /// Dims the content layer behind a transient (palette, approval card).
    func dimmed(_ active: Bool) -> some View {
        overlay(Color.black.opacity(active ? 0.28 : 0).allowsHitTesting(false))
            .animation(GH.motion, value: active)
    }
}

/// Transition that respects Reduce Motion.
public struct GHTransition: ViewModifier {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    let edge: Edge
    public func body(content: Content) -> some View {
        content.transition(reduceMotion ? .opacity : .move(edge: edge).combined(with: .opacity))
    }
}

public extension View {
    func ghTransition(from edge: Edge) -> some View { modifier(GHTransition(edge: edge)) }
}
