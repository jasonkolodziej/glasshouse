//! The window, in the design spec's layers:
//!   wallpaper    — the light behind the glass (a stand-in for the desktop in snapshots)
//!   L2 controls  — toolbar, tab strip and sidebar: frosted glass over the wallpaper
//!   L1 content   — the terminal pane: the theme's background at its opacity, over a blur
//!   L3 transient — command palette, approval card, find bar, Settings: glass with dimming
//!   L4 trusted   — trust capsule and the card's header: kernel state only, never program text

mod grid;
mod overlays;
mod settings;

use design_system::{Rgba, TrustState, TrustTint, gh};
use rune::color::srgb_decode;
use rune::{Color, Cx, El, TextStyle, col, el, oklch, oklcha, row, text};
use shell_integration::CommandStatus;

use crate::app::{App, SystemEnv};
use crate::hosts::Credential;
use crate::shortcuts::{self, Shortcut};

/// The mono slot: the theme's font is tried first (see `grid::family`), then these.
pub const MONO_FAMILIES: &str =
    "JetBrains Mono, SF Mono, Menlo, DejaVu Sans Mono, ui-monospace, monospace";

pub const TOOLBAR_HEIGHT: f32 = 52.0;
/// Space between the window edge, the sidebar and the pane.
pub const MARGIN: f32 = 10.0;
pub const PANE_RADIUS: f32 = 16.0;

/// An sRGB design-system colour as Rune's linear colour.
pub fn color(c: Rgba) -> Color {
    Color {
        r: srgb_decode(c.r),
        g: srgb_decode(c.g),
        b: srgb_decode(c.b),
        a: c.a,
    }
}

/// The chrome's colours for one appearance. Glass tints are translucent: the blur shows through.
#[derive(Clone, Copy, Debug)]
pub struct Ink {
    pub dark: bool,
    pub text: Color,
    pub secondary: Color,
    pub tertiary: Color,
    pub hairline: Color,
    pub rim: Color,
    pub glass: Color,
    pub glass_strong: Color,
    pub hover: Color,
    pub selected: Color,
    pub accent: Color,
    pub orange: Color,
    pub red: Color,
    pub green: Color,
    pub shadow: Color,
}

impl Ink {
    pub fn new(dark: bool) -> Ink {
        if dark {
            Ink {
                dark,
                text: oklch(0.96, 0.005, 265.0),
                secondary: oklcha(0.92, 0.01, 265.0, 0.62),
                tertiary: oklcha(0.92, 0.01, 265.0, 0.38),
                hairline: Color::WHITE.alpha(0.10),
                rim: Color::WHITE.alpha(0.16),
                glass: oklcha(0.24, 0.02, 270.0, 0.42),
                glass_strong: oklcha(0.2, 0.02, 270.0, 0.62),
                hover: Color::WHITE.alpha(0.06),
                selected: Color::WHITE.alpha(0.13),
                accent: oklch(0.66, 0.17, 252.0),
                orange: oklch(0.74, 0.16, 62.0),
                red: oklch(0.66, 0.2, 25.0),
                green: oklch(0.74, 0.15, 150.0),
                shadow: Color {
                    r: 0.0,
                    g: 0.0,
                    b: 0.02,
                    a: 0.45,
                },
            }
        } else {
            Ink {
                dark,
                text: oklch(0.2, 0.01, 265.0),
                secondary: oklcha(0.25, 0.02, 265.0, 0.62),
                tertiary: oklcha(0.25, 0.02, 265.0, 0.4),
                hairline: Color::BLACK.alpha(0.08),
                rim: Color::WHITE.alpha(0.7),
                glass: oklcha(0.98, 0.006, 265.0, 0.5),
                glass_strong: oklcha(0.985, 0.006, 265.0, 0.72),
                hover: Color::BLACK.alpha(0.04),
                selected: Color::BLACK.alpha(0.08),
                accent: oklch(0.55, 0.19, 255.0),
                orange: oklch(0.64, 0.17, 55.0),
                red: oklch(0.56, 0.21, 25.0),
                green: oklch(0.58, 0.15, 150.0),
                shadow: Color {
                    r: 0.05,
                    g: 0.06,
                    b: 0.12,
                    a: 0.22,
                },
            }
        }
    }

    pub fn tint(&self, tint: TrustTint) -> Color {
        match tint {
            TrustTint::Secondary => self.secondary,
            TrustTint::Accent => self.accent,
            TrustTint::Orange => self.orange,
        }
    }
}

pub fn style(size: f32, weight: f32) -> TextStyle {
    TextStyle::new(size, weight)
}

/// A frosted surface: blur what's behind, tint it, and draw a light rim on the edge.
pub fn glass(e: El, ink: &Ink, radius: f32, sigma: f32, tint: Color) -> El {
    e.radius(radius)
        .backdrop_blur(sigma)
        .fill(tint)
        .border(gh::glass_fallback::BORDER_WIDTH * 2.0, ink.rim)
}

/// A small borderless button: a glyph or a word on a hover fill.
pub fn icon_button(key: &str, glyph: &str, label: &str, ink: &Ink) -> El {
    row()
        .key(key)
        .role("button")
        .label(label)
        .hit()
        .size(30.0, 30.0)
        .center()
        .radius(9.0)
        .hover(ink.hover)
        .child(text(glyph).font(style(15.0, 500.0)).color(ink.secondary))
}

/// A capsule push button.
pub fn button(key: &str, label: &str, ink: &Ink, prominent: bool) -> El {
    let (fill, fg) = if prominent {
        (ink.accent, Color::WHITE)
    } else {
        (ink.selected, ink.text)
    };
    row()
        .key(key)
        .role("button")
        .label(label)
        .hit()
        .h(28.0)
        .px(14.0)
        .center()
        .radius(14.0)
        .fill(fill)
        .border(0.5, ink.hairline)
        .child(text(label.to_owned()).font(style(12.5, 550.0)).color(fg))
}

/// A pill for kinds and badges.
pub fn pill(label: impl Into<String>, fg: Color, bg: Color) -> El {
    row()
        .h(18.0)
        .px(7.0)
        .center()
        .radius(9.0)
        .fill(bg)
        .child(text(label.into()).font(style(10.5, 600.0)).color(fg))
}

pub fn view(app: &App, cx: &mut Cx<App>) -> El {
    // The theme follows the system unless a scene set it; poll applies a change.
    let system = SystemEnv {
        dark: cx.system_dark(),
        increase_contrast: app.env.increase_contrast,
    };
    if app.is_live() && system != app.env {
        app.seen_env.set(Some(system));
    }
    let ink = Ink::new(app.env.dark);
    let inset = cx.titlebar_inset();

    let body = row()
        .key("body")
        .w_full()
        .grow(1.0)
        .px(MARGIN)
        .pb(MARGIN)
        .gap(MARGIN)
        .children(app.show_sidebar.then(|| sidebar(app, &ink)))
        .child(grid::pane(app, &ink));

    let mut root = el()
        .key("root")
        .w_full()
        .h_full()
        .on_layout(|s: &mut App, r: [f32; 4]| s.window = (r[2], r[3]))
        .child(wallpaper(app.window, ink.dark))
        .child(
            col()
                .w_full()
                .h_full()
                .child(toolbar(app, &ink, inset))
                .child(body),
        )
        .child(grid::cell_probe(app));
    if app.find.is_some() {
        root = root.child(overlays::find_bar(app, &ink));
    }
    if app.palette.is_some() {
        root = root.child(overlays::dim("palette-dim", |s: &mut App, tx| {
            s.close_palette(tx)
        }));
        root = root.child(overlays::palette(app, &ink));
    }
    if app.settings.is_some() {
        root = root.child(overlays::dim("settings-dim", |s: &mut App, _| {
            s.settings = None
        }));
        root = root.child(settings::sheet(app, &ink));
    }
    root
}

/// Soft coloured light, as a wallpaper seen through the glass would give.
fn wallpaper((w, h): (f32, f32), dark: bool) -> El {
    let (w, h) = if w > 0.0 { (w, h) } else { (1180.0, 740.0) };
    let base = if dark {
        oklch(0.17, 0.035, 278.0)
    } else {
        oklch(0.92, 0.03, 255.0)
    };
    let l = if dark { 0.58 } else { 0.82 };
    let a = if dark { 0.85 } else { 0.9 };
    let blobs = [
        (0.08, 0.15, 0.55, oklcha(l, 0.19, 300.0, a)),
        (0.85, 0.05, 0.5, oklcha(l + 0.04, 0.14, 200.0, a)),
        (0.6, 0.95, 0.6, oklcha(l - 0.04, 0.2, 262.0, a)),
        (0.05, 0.95, 0.42, oklcha(l + 0.03, 0.18, 350.0, a * 0.85)),
        (0.42, 0.42, 0.32, oklcha(l, 0.15, 170.0, a * 0.55)),
    ];
    let lights = blobs.into_iter().map(move |(x, y, r, c)| {
        let r = r * w.max(h);
        el().absolute(x * w - r, y * h - r)
            .size(2.0 * r, 2.0 * r)
            .radial_gradient(
                [0.5, 0.5],
                r,
                [(0.0, c), (0.45, c.alpha(c.a * 0.45)), (1.0, c.alpha(0.0))],
            )
    });
    el().key("wallpaper")
        .absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .fill(base)
        .children(lights)
}

// MARK: Toolbar

fn toolbar(app: &App, ink: &Ink, inset: f32) -> El {
    let sidebar_toggle = icon_button("toggle-sidebar", "◧", "Toggle Sessions Sidebar", ink)
        .help(format!(
            "Toggle Sidebar ({})",
            shortcuts::label(Shortcut::ToggleSidebar)
        ))
        .on_press(|s: &mut App| s.show_sidebar = !s.show_sidebar);
    let new_tab = icon_button("new-tab", "+", "New Tab", ink)
        .help(format!("New Tab ({})", shortcuts::label(Shortcut::NewTab)))
        .on_press(|s: &mut App| s.new_tab());
    let palette = icon_button("open-palette", "⌕", "Command Palette", ink)
        .help(format!(
            "Command Palette ({})",
            shortcuts::label(Shortcut::CommandPalette)
        ))
        .on_press_tx(|s: &mut App, tx| s.open_palette(tx));
    let trust = app.selected().map_or(TrustState::Idle, |s| s.trust.clone());

    row()
        .key("toolbar")
        .w_full()
        .h(TOOLBAR_HEIGHT)
        .pl(MARGIN + inset)
        .pr(MARGIN)
        .gap(6.0)
        .center_y()
        .window_drag()
        .child(sidebar_toggle)
        .child(el().grow(1.0))
        .children((!app.sessions.is_empty()).then(|| tab_strip(app, ink)))
        .child(el().grow(1.0))
        .child(palette)
        .child(new_tab)
        .children(trust.is_visible().then(|| trust_capsule(&trust, ink)))
}

/// Tabs in one glass capsule. The active tab is fill and weight; tint is kept for trust signals.
/// Titles are plain text: program-set strings must never look like the trust capsule.
fn tab_strip(app: &App, ink: &Ink) -> El {
    let multiple = app.sessions.len() > 1;
    let tabs = app.sessions.iter().map(|s| {
        let id = s.id;
        let active = multiple && Some(id) == app.selected;
        let hovered = app.hovered_tab == Some(id);
        let closable = multiple && hovered;
        let status = match s.timeline.current().map(|r| r.status()) {
            _ if s.exited && s.exit_code != Some(0) => Some(("✕", ink.red)),
            Some(CommandStatus::Running) => Some(("◌", ink.accent)),
            Some(CommandStatus::Failed) => Some(("●", ink.red)),
            _ => None,
        };
        let close = closable.then(|| {
            row()
                .key(format!("close-{id}"))
                .role("button")
                .label("Close Tab")
                .hit()
                .size(16.0, 16.0)
                .center()
                .radius(8.0)
                .hover(ink.selected)
                .on_press(move |s: &mut App| s.close(id))
                .child(text("×").font(style(12.0, 600.0)).color(ink.secondary))
        });
        let mut tab = row()
            .key(format!("tab-{id}"))
            .role("tab")
            .label(s.title.clone())
            .hit()
            .h(gh::size::TAB_HEIGHT)
            .pl(if closable { 6.0 } else { gh::space::M })
            .pr(gh::space::M)
            .gap(gh::space::XS + 2.0)
            .center_y()
            .radius(gh::size::TAB_HEIGHT / 2.0)
            .on_press(move |s: &mut App| s.select(id))
            .on_hover_change(move |s: &mut App, inside, _| {
                if inside {
                    s.hovered_tab = Some(id);
                } else if s.hovered_tab == Some(id) {
                    s.hovered_tab = None;
                }
            })
            .children(close)
            .children(status.map(|(g, c)| text(g).font(style(10.0, 700.0)).color(c)))
            .child(
                text(s.title.clone())
                    .font(style(12.5, if active { 650.0 } else { 450.0 }))
                    .color(if active || !multiple {
                        ink.text
                    } else {
                        ink.secondary
                    })
                    .truncate()
                    .max_w(170.0),
            );
        if active {
            tab = tab.fill(ink.selected);
        } else if hovered && multiple {
            tab = tab.fill(ink.hover);
        }
        tab
    });
    glass(
        row()
            .key("tabs")
            .padding(gh::size::TAB_GROUP_INSET)
            .gap(gh::size::TAB_SPACING)
            .center_y()
            .max_w(760.0),
        ink,
        gh::radius::CAPSULE,
        20.0,
        ink.glass,
    )
    .children(tabs)
}

/// L4: filled and tinted, from kernel state only.
fn trust_capsule(trust: &TrustState, ink: &Ink) -> El {
    let fill = ink.tint(trust.tint());
    let glyph = match trust {
        TrustState::SecureInput => "●",
        TrustState::TouchIdSudo => "◉",
        TrustState::RemoteSession(_) => "⇄",
        TrustState::Idle => "",
    };
    row()
        .key("trust")
        .role("status")
        .label(trust.accessibility_description())
        .h(28.0)
        .px(12.0)
        .gap(6.0)
        .center_y()
        .radius(14.0)
        .fill(fill)
        .shadow(2.0, 10.0, fill.alpha(0.45))
        .child(text(glyph).font(style(12.0, 700.0)).color(Color::WHITE))
        .child(
            text(trust.label())
                .font(style(12.5, 650.0))
                .color(Color::WHITE),
        )
}

// MARK: Sidebar

fn section_title(title: &str, ink: &Ink) -> El {
    text(title.to_uppercase())
        .font(style(10.5, 650.0).tracking(0.6))
        .color(ink.tertiary)
        .px(10.0)
        .pt(10.0)
        .pb(4.0)
}

fn sidebar(app: &App, ink: &Ink) -> El {
    let sessions = app.sessions.iter().map(|s| {
        let id = s.id;
        let selected = Some(id) == app.selected;
        let subtitle = s.cwd.as_deref().map(home_relative).unwrap_or_default();
        let mut r = row()
            .key(format!("side-tab-{id}"))
            .role("button")
            .label(s.title.clone())
            .hit()
            .w_full()
            .px(10.0)
            .py(7.0)
            .gap(10.0)
            .center_y()
            .radius(10.0)
            .hover(ink.hover)
            .on_press(move |s: &mut App| s.select(id))
            .child(
                row()
                    .size(26.0, 26.0)
                    .center()
                    .radius(7.0)
                    .fill(if selected { ink.accent } else { ink.selected })
                    .child(
                        text(">_")
                            .font(style(10.5, 700.0).family(rune::tok::Family::Mono))
                            .color(if selected {
                                Color::WHITE
                            } else {
                                ink.secondary
                            }),
                    ),
            )
            .child(
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .gap(1.0)
                    .child(
                        text(s.title.clone())
                            .font(style(13.0, 550.0))
                            .color(ink.text)
                            .truncate(),
                    )
                    .children((!subtitle.is_empty()).then(|| {
                        text(subtitle)
                            .font(style(11.0, 450.0))
                            .color(ink.secondary)
                            .truncate_head()
                    })),
            );
        if selected {
            r = r.fill(ink.selected);
        }
        r
    });
    let hosts = app.hosts.iter().map(|h| {
        let alias = h.alias.clone();
        let subtitle = [h.user.as_deref(), h.host_name.as_deref()]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join("@");
        let badge_bg = match h.credential {
            Credential::SecureEnclave => ink.accent.alpha(0.22),
            Credential::Agent => ink.green.alpha(0.22),
            Credential::Other => ink.selected,
        };
        row()
            .key(format!("host-{alias}"))
            .role("button")
            .label(format!("Connect to {alias}"))
            .help(h.credential.help())
            .hit()
            .w_full()
            .px(10.0)
            .py(7.0)
            .gap(10.0)
            .center_y()
            .radius(10.0)
            .hover(ink.hover)
            .on_press(move |s: &mut App| s.open_host(&alias))
            .child(
                row()
                    .size(26.0, 26.0)
                    .center()
                    .radius(7.0)
                    .fill(ink.selected)
                    .child(text("⇄").font(style(12.0, 600.0)).color(ink.secondary)),
            )
            .child(
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .gap(1.0)
                    .child(
                        text(h.alias.clone())
                            .font(style(13.0, 550.0))
                            .color(ink.text)
                            .truncate(),
                    )
                    .child(
                        text(subtitle)
                            .font(style(11.0, 450.0))
                            .color(ink.secondary)
                            .truncate(),
                    ),
            )
            .child(pill(h.credential.badge(), ink.text, badge_bg))
    });
    let empty_hosts = app.hosts.is_empty().then(|| {
        text("No hosts in ~/.ssh/config")
            .font(style(12.0, 450.0))
            .color(ink.secondary)
            .px(10.0)
            .py(6.0)
    });
    glass(
        col()
            .key("sidebar")
            .w(gh::size::SIDEBAR_WIDTH)
            .h_full()
            .padding(8.0)
            .gap(2.0),
        ink,
        PANE_RADIUS,
        30.0,
        ink.glass,
    )
    .child(section_title("Open Sessions", ink))
    .children(sessions)
    .child(section_title("Hosts", ink))
    .children(hosts)
    .children(empty_hosts)
    .child(el().grow(1.0))
    .child(
        row()
            .key("sidebar-settings")
            .role("button")
            .label("Settings")
            .hit()
            .w_full()
            .px(10.0)
            .py(8.0)
            .gap(8.0)
            .center_y()
            .radius(10.0)
            .hover(ink.hover)
            .on_press(|s: &mut App| s.open_settings(crate::app::SettingsPane::Security))
            .child(text("◎").font(style(13.0, 500.0)).color(ink.secondary))
            .child(
                text("Settings")
                    .font(style(12.5, 500.0))
                    .color(ink.secondary),
            )
            .child(el().grow(1.0))
            .child(
                text(shortcuts::label(Shortcut::Settings))
                    .font(style(11.0, 500.0))
                    .color(ink.tertiary),
            ),
    )
}

/// `/Users/me/src` → `~/src`.
pub fn home_relative(path: &str) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    match path.strip_prefix(&home) {
        Some(rest) if !home.is_empty() && (rest.is_empty() || rest.starts_with('/')) => {
            format!("~{rest}")
        }
        _ => path.to_owned(),
    }
}
