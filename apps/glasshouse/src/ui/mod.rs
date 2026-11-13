//! The window, drawn the way a TUI draws itself: one monospace font, the terminal theme's own
//! colours for the chrome, 1 px rules instead of cards, square corners, and an accent bar on
//! whatever has focus.
//!
//!   title row    — `• tab` entries (accent bar over the active one) and the trust block
//!   body         — sidebar │ pane (header, grid, status line) │ icon rail
//!   hint bar     — numbered tabs (⌘1…9) on the left, key hints on the right
//!   L3 overlays  — palette, approval card, find, Settings: boxed, titled in their top rule
//!   L4 trusted   — the trust block and the card's header: kernel state only, never program text
//!
//! Below 100% window opacity the theme background turns translucent and a wallpaper with a
//! backdrop blur shows through: the same frosted technique as Rune's `frosted-bar` example.

mod grid;

pub use grid::strip_prompt;
mod overlays;
mod settings;

use design_system::{Rgba, TrustState, TrustTint};
use rune::color::srgb_decode;
use rune::tok::Family;
use rune::{Color, Cx, El, TextStyle, col, el, oklch, oklcha, row, text};
use shell_integration::CommandStatus;
use terminal_core::TerminalAppearance;

use crate::app::{App, SettingsPane, SystemEnv};
use crate::hosts::Credential;
use crate::shortcuts::{self, Shortcut};

/// The mono slot: the theme's font is tried first (see `grid::family`), then these.
pub const MONO_FAMILIES: &str =
    "JetBrains Mono, SF Mono, Menlo, DejaVu Sans Mono, ui-monospace, monospace";

pub const TITLE_HEIGHT: f32 = 34.0;
pub const BAR_HEIGHT: f32 = 26.0;
pub const RAIL_WIDTH: f32 = 40.0;
pub const SIDEBAR_WIDTH: f32 = 260.0;
/// Below this window opacity the chrome turns to frosted glass over the wallpaper.
const GLASS_BELOW: f32 = 0.999;
/// The frame's blur, in points (CSS `backdrop-filter: blur()`).
const GLASS_BLUR: f32 = 40.0;

/// An sRGB design-system colour as Rune's linear colour.
pub fn color(c: Rgba) -> Color {
    Color {
        r: srgb_decode(c.r),
        g: srgb_decode(c.g),
        b: srgb_decode(c.b),
        a: c.a,
    }
}

/// The chrome's colours, taken from the terminal theme so the frame and the grid are one surface.
#[derive(Clone, Copy, Debug)]
pub struct Ink {
    pub dark: bool,
    /// Translucent chrome over a blurred wallpaper (opacity below 100%).
    pub glass: bool,
    pub opacity: f32,
    pub bg: Color,
    /// A step lighter (dark) or darker (light) than `bg`: bars and headers.
    pub panel: Color,
    pub text: Color,
    pub secondary: Color,
    pub tertiary: Color,
    pub hairline: Color,
    pub rule: Color,
    pub hover: Color,
    pub selected: Color,
    pub accent: Color,
    pub on_accent: Color,
    pub red: Color,
    pub green: Color,
    pub orange: Color,
    pub blue: Color,
    pub shadow: Color,
}

impl Ink {
    pub fn new(a: &TerminalAppearance, dark: bool) -> Ink {
        let bg = color(a.background).alpha(1.0);
        let fg = color(a.foreground).alpha(1.0);
        let ansi = |i: usize| color(a.ansi[i]).alpha(1.0);
        let opacity = a.background_opacity.clamp(0.3, 1.0);
        // Blend in Oklab, as the eye sees steps; linear light jumps too bright near black.
        let (b, f) = (bg.to_oklab(), fg.to_oklab());
        let mix = |t: f32| Color::from_oklab(std::array::from_fn(|i| b[i] + (f[i] - b[i]) * t));
        Ink {
            dark,
            glass: opacity < GLASS_BELOW,
            opacity,
            bg,
            panel: mix(if dark { 0.06 } else { 0.05 }),
            text: fg,
            secondary: mix(0.58),
            tertiary: mix(0.36),
            hairline: mix(0.13),
            rule: mix(0.22),
            hover: mix(0.08),
            selected: mix(0.14),
            accent: ansi(1),
            on_accent: bg,
            red: ansi(1),
            green: ansi(2),
            orange: ansi(3),
            blue: ansi(4),
            shadow: Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: if dark { 0.55 } else { 0.18 },
            },
        }
    }

    pub fn tint(&self, tint: TrustTint) -> Color {
        match tint {
            TrustTint::Secondary => self.secondary,
            TrustTint::Accent => self.blue,
            TrustTint::Orange => self.orange,
        }
    }

    /// A surface's fill: the colour itself, or translucent when the chrome is glass.
    pub fn surface(&self, c: Color) -> Color {
        if self.glass { c.alpha(self.opacity) } else { c }
    }
}

/// UI text: monospace, like every other glyph in the window.
pub fn style(size: f32, weight: f32) -> TextStyle {
    TextStyle::new(size, weight).family(Family::Mono)
}

pub const UI: f32 = 12.5;

/// A boxed surface: a 1 px rule round it, square corners, frosted when the chrome is glass.
pub fn panel(e: El, ink: &Ink, fill: Color) -> El {
    let e = e.fill(ink.surface(fill)).border(1.0, ink.rule);
    if ink.glass { e.backdrop_blur(24.0) } else { e }
}

/// A box with its title cut into the top rule: `┤ title ├`.
pub fn titled(e: El, title: &str, ink: &Ink) -> El {
    panel(e, ink, ink.bg).shadow(14.0, 40.0, ink.shadow).child(
        text(format!(" {title} "))
            .font(style(UI, 600.0))
            .color(ink.accent)
            .fill(ink.bg)
            .px(4.0)
            .absolute(12.0, -9.0),
    )
}

/// A one-glyph button in the rail.
pub fn icon_button(key: &str, glyph: &str, label: &str, ink: &Ink) -> El {
    row()
        .key(key)
        .role("button")
        .label(label)
        .hit()
        .size(30.0, 28.0)
        .center()
        .hover(ink.selected)
        .child(text(glyph).font(style(14.0, 500.0)).color(ink.secondary))
}

/// `[ label ]`: prominent buttons take the accent.
pub fn button(key: &str, label: &str, ink: &Ink, prominent: bool) -> El {
    let fg = if prominent { ink.accent } else { ink.text };
    row()
        .key(key)
        .role("button")
        .label(label)
        .hit()
        .h(24.0)
        .px(2.0)
        .center()
        .hover(ink.selected)
        .child(text("[ ").font(style(UI, 500.0)).color(ink.tertiary))
        .child(text(label.to_owned()).font(style(UI, 600.0)).color(fg))
        .child(text(" ]").font(style(UI, 500.0)).color(ink.tertiary))
}

/// `[label]` for kinds and badges.
pub fn pill(label: impl Into<String>, fg: Color, _bg: Color) -> El {
    text(format!("[{}]", label.into()))
        .font(style(11.5, 600.0))
        .color(fg)
}

/// A rule across the available width.
pub fn hrule(ink: &Ink) -> El {
    el().w_full().h(1.0).fill(ink.hairline)
}

pub fn vrule(ink: &Ink) -> El {
    el().w(1.0).h_full().fill(ink.hairline)
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
    let appearance = app
        .selected()
        .map_or_else(|| app.current_appearance(), |s| s.appearance.clone());
    let ink = Ink::new(&appearance, app.env.dark);
    let inset = cx.titlebar_inset();

    let body = row()
        .key("body")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .children(app.show_sidebar.then(|| sidebar(app, &ink)))
        .children(app.show_sidebar.then(|| vrule(&ink)))
        .child(grid::pane(app, &ink))
        .child(vrule(&ink))
        .child(rail(&ink));

    let frame = col()
        .w_full()
        .h_full()
        .child(title_row(app, &ink, inset))
        .child(hrule(&ink))
        .child(body)
        .child(hrule(&ink))
        .child(hint_bar(app, &ink));

    // The window's contents: frost, frame and overlays. Overlays place themselves by this
    // layer's size, so it reports it.
    let mut window = el()
        .key("window")
        .w_full()
        .h_full()
        .on_layout(|s: &mut App, r: [f32; 4]| s.window = (r[2], r[3]));
    window = if ink.glass {
        // One blur for the whole frame: every translucent surface above sits on frosted glass,
        // and the other blur slots stay free for the palette, card and Settings.
        window.child(
            el().key("frost")
                .absolute(0.0, 0.0)
                .w_full()
                .h_full()
                .backdrop_blur(GLASS_BLUR),
        )
    } else {
        window.fill(ink.bg)
    };
    window = window.child(frame).child(grid::cell_probe(app));
    if app.showcase {
        window = window.child(traffic_lights());
    }
    if app.palette.is_some() {
        window = window.child(overlays::dim("palette-dim", &ink, |s: &mut App, tx| {
            s.close_palette(tx)
        }));
        window = window.child(overlays::palette(app, &ink));
    }
    if app.settings.is_some() {
        window = window.child(overlays::dim("settings-dim", &ink, |s: &mut App, _| {
            s.settings = None
        }));
        window = window.child(settings::sheet(app, &ink));
    }

    let mut root = el().key("root").w_full().h_full();
    if ink.glass || app.showcase {
        root = root.child(wallpaper(app.window, ink.dark));
    }
    if app.showcase {
        // Presentation: the window floats on the wallpaper, so the glass has a desktop to show.
        let m = SHOWCASE_MARGIN;
        root.child(
            el().key("showcase")
                .pin(Some(m), Some(m * 0.8), Some(m), Some(m * 1.2))
                .radius(12.0)
                .clip_rounded(12.0)
                .shadow(
                    30.0,
                    80.0,
                    Color {
                        r: 0.0,
                        g: 0.0,
                        b: 0.02,
                        a: 0.6,
                    },
                )
                .border(1.0, Color::WHITE.alpha(if ink.dark { 0.14 } else { 0.5 }))
                .child(window),
        )
    } else {
        root.child(window)
    }
}

const SHOWCASE_MARGIN: f32 = 56.0;

/// The three window buttons, drawn for the presentation scene (a live window gets the system's).
fn traffic_lights() -> El {
    let dot = |c: Color| {
        el().size(12.0, 12.0)
            .radius(6.0)
            .fill(c)
            .border(0.5, Color::BLACK.alpha(0.2))
    };
    row()
        .key("traffic-lights")
        .absolute(14.0, 11.0)
        .gap(8.0)
        .child(dot(rune::color::hex("#ff5f57").unwrap_or(Color::WHITE)))
        .child(dot(rune::color::hex("#febc2e").unwrap_or(Color::WHITE)))
        .child(dot(rune::color::hex("#28c840").unwrap_or(Color::WHITE)))
}

/// Soft coloured light, seen through the glass when the window is translucent.
fn wallpaper((w, h): (f32, f32), dark: bool) -> El {
    let (w, h) = if w > 0.0 { (w, h) } else { crate::SIZE };
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

// MARK: Title row

/// `• title` per tab, like an editor's buffer list. The active tab is bright, with an accent bar
/// over it; tint elsewhere is kept for trust. Titles are plain text: program-set strings must
/// never look like the trust block.
fn title_row(app: &App, ink: &Ink, inset: f32) -> El {
    let trust = app.selected().map_or(TrustState::Idle, |s| s.trust.clone());
    let tabs = app.sessions.iter().map(|s| {
        let id = s.id;
        let active = Some(id) == app.selected;
        let hovered = app.hovered_tab == Some(id);
        let (glyph, glyph_color) = match s.timeline.current().map(|r| r.status()) {
            _ if hovered && app.sessions.len() > 1 => ("×", ink.text),
            _ if s.exited && s.exit_code != Some(0) => ("✕", ink.red),
            Some(CommandStatus::Running) => ("◌", ink.blue),
            Some(CommandStatus::Failed) => ("•", ink.red),
            _ => ("•", if active { ink.accent } else { ink.tertiary }),
        };
        let bullet = text(glyph).font(style(UI, 700.0)).color(glyph_color);
        let bullet = if hovered && app.sessions.len() > 1 {
            bullet
                .key(format!("close-{id}"))
                .role("button")
                .label("Close Tab")
                .hit()
                .on_press(move |s: &mut App| s.close(id))
        } else {
            bullet
        };
        col()
            .key(format!("tab-{id}"))
            .role("tab")
            .label(s.title.clone())
            .hit()
            .h_full()
            .on_press(move |s: &mut App| s.select(id))
            .on_hover_change(move |s: &mut App, inside, _| {
                if inside {
                    s.hovered_tab = Some(id);
                } else if s.hovered_tab == Some(id) {
                    s.hovered_tab = None;
                }
            })
            .child(el().w_full().h(2.0).fill(if active {
                ink.accent
            } else {
                Color::TRANSPARENT
            }))
            .child(
                row()
                    .grow(1.0)
                    .px(12.0)
                    .gap(8.0)
                    .center_y()
                    .child(bullet)
                    .child(
                        text(s.title.clone())
                            .font(style(UI, if active { 600.0 } else { 450.0 }))
                            .color(if active { ink.text } else { ink.secondary })
                            .truncate()
                            .max_w(220.0),
                    ),
            )
    });
    row()
        .key("toolbar")
        .w_full()
        .h(TITLE_HEIGHT)
        .pl(8.0 + inset)
        .pr(8.0)
        .center_y()
        .fill(ink.surface(ink.panel))
        .window_drag()
        .children(tabs)
        .child(el().grow(1.0))
        .children(trust.is_visible().then(|| trust_block(&trust, ink)))
}

/// L4: an inverted block in the trust colour, from kernel state only.
fn trust_block(trust: &TrustState, ink: &Ink) -> El {
    let fill = ink.tint(trust.tint());
    row()
        .key("trust")
        .role("status")
        .label(trust.accessibility_description())
        .h(22.0)
        .px(10.0)
        .center_y()
        .fill(fill)
        .child(
            text(trust.label().to_uppercase())
                .font(style(11.5, 700.0))
                .color(ink.on_accent),
        )
}

// MARK: Rail and hint bar

fn rail(ink: &Ink) -> El {
    let item = |key: &str, glyph: &str, label: &str, shortcut: Shortcut| {
        icon_button(key, glyph, label, ink)
            .help(format!("{label} ({})", shortcuts::label(shortcut)))
    };
    col()
        .key("rail")
        .w(RAIL_WIDTH)
        .h_full()
        .py(8.0)
        .gap(4.0)
        .items_center()
        .fill(ink.surface(ink.panel))
        .child(
            item(
                "open-palette",
                "⌘",
                "Command Palette",
                Shortcut::CommandPalette,
            )
            .on_press_tx(|s: &mut App, tx| s.open_palette(tx)),
        )
        .child(
            item("new-tab", "+", "New Tab", Shortcut::NewTab).on_press(|s: &mut App| s.new_tab()),
        )
        .child(
            item(
                "toggle-sidebar",
                "◧",
                "Toggle Sidebar",
                Shortcut::ToggleSidebar,
            )
            .on_press(|s: &mut App| s.show_sidebar = !s.show_sidebar),
        )
        .child(
            item("find", "/", "Find", Shortcut::Find)
                .on_press_tx(|s: &mut App, tx| s.run_shortcut(Shortcut::Find, tx)),
        )
        .child(
            item(
                "copy-output",
                "▤",
                "Copy Last Output",
                Shortcut::CopyLastOutput,
            )
            .on_press_tx(|s: &mut App, tx| s.run_shortcut(Shortcut::CopyLastOutput, tx)),
        )
        .child(
            item("clear", "⌫", "Clear", Shortcut::Clear)
                .on_press_tx(|s: &mut App, tx| s.run_shortcut(Shortcut::Clear, tx)),
        )
        .child(el().grow(1.0))
        .child(
            item("open-settings", "◎", "Settings", Shortcut::Settings)
                .on_press(|s: &mut App| s.open_settings(SettingsPane::Security)),
        )
}

/// Numbered tabs (the ⌘1…9 targets) and the keys worth knowing.
fn hint_bar(app: &App, ink: &Ink) -> El {
    let numbered = app.sessions.iter().enumerate().take(9).map(|(i, s)| {
        let id = s.id;
        let active = Some(id) == app.selected;
        row()
            .key(format!("slot-{id}"))
            .role("button")
            .label(format!("Tab {}: {}", i + 1, s.title))
            .hit()
            .h_full()
            .px(8.0)
            .gap(6.0)
            .center_y()
            .hover(ink.hover)
            .on_press(move |s: &mut App| s.select(id))
            .child(
                text(format!("{}", i + 1))
                    .font(style(UI, 700.0))
                    .color(if active { ink.accent } else { ink.tertiary }),
            )
            .child(
                text(
                    s.cwd
                        .as_deref()
                        .map(home_relative)
                        .unwrap_or_else(|| s.title.clone()),
                )
                .font(style(UI, 450.0))
                .color(if active { ink.green } else { ink.tertiary })
                .truncate()
                .max_w(200.0),
            )
    });
    let hint = |shortcut: Shortcut, label: &str| {
        row()
            .gap(5.0)
            .center_y()
            .child(
                text(shortcuts::label(shortcut))
                    .font(style(11.5, 600.0))
                    .color(ink.secondary),
            )
            .child(
                text(label.to_owned())
                    .font(style(11.5, 450.0))
                    .color(ink.tertiary),
            )
    };
    row()
        .key("hints")
        .w_full()
        .h(BAR_HEIGHT)
        .px(6.0)
        .gap(4.0)
        .center_y()
        .fill(ink.surface(ink.panel))
        .children(numbered)
        .child(el().grow(1.0))
        .child(
            row()
                .gap(16.0)
                .pr(8.0)
                .center_y()
                .child(hint(Shortcut::CommandPalette, "palette"))
                .child(hint(Shortcut::NewTab, "new"))
                .child(hint(Shortcut::Find, "find"))
                .child(hint(Shortcut::Settings, "settings")),
        )
}

// MARK: Sidebar

fn section_title(title: &str, ink: &Ink) -> El {
    row()
        .w_full()
        .px(12.0)
        .pt(12.0)
        .pb(6.0)
        .gap(8.0)
        .center_y()
        .child(
            text(title.to_lowercase())
                .font(style(11.5, 650.0))
                .color(ink.accent),
        )
        .child(el().grow(1.0).h(1.0).fill(ink.hairline))
}

fn sidebar(app: &App, ink: &Ink) -> El {
    let sessions = app.sessions.iter().enumerate().map(|(i, s)| {
        let id = s.id;
        let selected = Some(id) == app.selected;
        let subtitle = s.cwd.as_deref().map(home_relative).unwrap_or_default();
        let mut r = row()
            .key(format!("side-tab-{id}"))
            .role("button")
            .label(s.title.clone())
            .hit()
            .w_full()
            .px(12.0)
            .py(4.0)
            .gap(10.0)
            .hover(ink.hover)
            .on_press(move |s: &mut App| s.select(id))
            .child(
                text(if selected { "▸" } else { " " })
                    .font(style(UI, 700.0))
                    .color(ink.accent),
            )
            .child(
                text(format!("{}", i + 1))
                    .font(style(UI, 600.0))
                    .color(ink.tertiary),
            )
            .child(
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .child(
                        text(s.title.clone())
                            .font(style(UI, if selected { 650.0 } else { 450.0 }))
                            .color(ink.text)
                            .truncate(),
                    )
                    .children((!subtitle.is_empty()).then(|| {
                        text(subtitle)
                            .font(style(11.5, 450.0))
                            .color(ink.tertiary)
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
        let badge = match h.credential {
            Credential::SecureEnclave => ink.blue,
            Credential::Agent => ink.green,
            Credential::Other => ink.tertiary,
        };
        row()
            .key(format!("host-{alias}"))
            .role("button")
            .label(format!("Connect to {alias}"))
            .help(h.credential.help())
            .hit()
            .w_full()
            .px(12.0)
            .py(4.0)
            .gap(10.0)
            .hover(ink.hover)
            .on_press(move |s: &mut App| s.open_host(&alias))
            .child(text("⇄").font(style(UI, 600.0)).color(ink.tertiary))
            .child(
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .child(
                        text(h.alias.clone())
                            .font(style(UI, 550.0))
                            .color(ink.text)
                            .truncate(),
                    )
                    .child(
                        text(subtitle)
                            .font(style(11.5, 450.0))
                            .color(ink.tertiary)
                            .truncate(),
                    ),
            )
            .child(pill(h.credential.badge(), badge, ink.hover))
    });
    let empty_hosts = app.hosts.is_empty().then(|| {
        text("no hosts in ~/.ssh/config")
            .font(style(11.5, 450.0))
            .color(ink.tertiary)
            .px(12.0)
            .py(4.0)
    });
    col()
        .key("sidebar")
        .w(SIDEBAR_WIDTH)
        .h_full()
        .fill(ink.surface(ink.bg))
        .child(section_title("sessions", ink))
        .children(sessions)
        .child(section_title("hosts", ink))
        .children(hosts)
        .children(empty_hosts)
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
