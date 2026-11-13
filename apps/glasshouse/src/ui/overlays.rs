//! L3: transient glass — the command palette, the approval card and the find bar — and the
//! dimming behind a modal one.

use design_system::gh;
use rune::tok::Family;
use rune::{Align, Color, El, Key, KeyEvent, Tx, col, el, row, text, text_input};

use super::{Ink, glass, pill, style};
use crate::app::App;
use crate::palette::PaletteItem;
use crate::session::{ApprovalContext, ApprovalPhase, ApprovalSource};

/// Dims everything behind a modal overlay; pressing it dismisses the overlay.
pub fn dim(key: &str, dismiss: fn(&mut App, &mut Tx)) -> El {
    el().key(key)
        .absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .hit()
        .fill(Color::BLACK.alpha(gh::DIM_OPACITY))
        .on_press_tx(move |s: &mut App, tx| dismiss(s, tx))
}

const PALETTE_ROWS: usize = 9;

pub fn palette(app: &App, ink: &Ink) -> El {
    let Some(state) = &app.palette else {
        return el();
    };
    let results = app.palette_results();
    let selection = state.selection.min(results.len().saturating_sub(1));
    // Keep the selected row in the window of rows shown.
    let first = selection.saturating_sub(PALETTE_ROWS - 1);

    let field = text_input(&state.edit, style(16.0, 450.0).lh(24.0))
        .key("palette-field")
        .single_line()
        .grow(1.0)
        .color(ink.text)
        .caret(ink.accent, ink.accent.alpha(0.3))
        .placeholder(
            "Search actions, hosts, tabs and recent commands",
            ink.tertiary,
        )
        .on_edit(|s: &mut App| match &mut s.palette {
            Some(p) => &mut p.edit,
            None => &mut s.scratch,
        })
        .on_input_change(|s: &mut App, _: &mut Tx| {
            if let Some(p) = &mut s.palette {
                p.selection = 0;
            }
        })
        .on_input_key(|s: &mut App, ev: &KeyEvent, tx: &mut Tx| match ev.key {
            Key::Down => {
                s.move_palette_selection(1);
                true
            }
            Key::Up => {
                s.move_palette_selection(-1);
                true
            }
            Key::Escape => {
                s.close_palette(tx);
                true
            }
            _ => false,
        })
        .on_submit(|s: &mut App, tx| s.run_palette_selection(tx));

    let rows = results
        .iter()
        .enumerate()
        .skip(first)
        .take(PALETTE_ROWS)
        .map(|(i, item)| palette_row(i, item, i == selection, ink));
    let count = results.len();
    let footer = row()
        .w_full()
        .px(16.0)
        .py(9.0)
        .gap(14.0)
        .center_y()
        .border_sides([0.5, 0.0, 0.0, 0.0], ink.hairline)
        .child(hint("↑↓", "Move", ink))
        .child(hint("↩", "Run", ink))
        .child(hint("esc", "Close", ink))
        .child(el().grow(1.0))
        .child(
            text(format!(
                "{count} result{}",
                if count == 1 { "" } else { "s" }
            ))
            .font(style(11.0, 500.0))
            .color(ink.tertiary),
        );

    let mut list = col()
        .key("palette-results")
        .w_full()
        .padding(6.0)
        .gap(2.0)
        .children(rows);
    if results.is_empty() {
        list = list.child(
            text("No matches")
                .font(style(13.0, 450.0))
                .color(ink.secondary)
                .padding(14.0),
        );
    }
    glass(
        col()
            .key("palette")
            .absolute(0.0, 0.0)
            .w(gh::size::PALETTE_WIDTH)
            .clip_rounded(gh::radius::PALETTE),
        ink,
        gh::radius::PALETTE,
        34.0,
        ink.glass_strong,
    )
    .shadow(24.0, 60.0, ink.shadow)
    .center_x_in_window(app.window.0, 64.0)
    .child(
        row()
            .w_full()
            .h(58.0)
            .px(18.0)
            .gap(12.0)
            .center_y()
            .border_sides([0.0, 0.0, 0.5, 0.0], ink.hairline)
            .child(text("⌕").font(style(20.0, 500.0)).color(ink.secondary))
            .child(field),
    )
    .child(list)
    .child(footer)
}

trait Place {
    fn center_x_in_window(self, window_width: f32, top: f32) -> Self;
}

impl Place for El {
    /// Centred horizontally `top` points from the window's top (the window width is from layout).
    fn center_x_in_window(self, window_width: f32, top: f32) -> El {
        let w = if window_width > 0.0 {
            window_width
        } else {
            1180.0
        };
        self.absolute(((w - gh::size::PALETTE_WIDTH) / 2.0).max(12.0), top)
    }
}

fn hint(key: &str, label: &str, ink: &Ink) -> El {
    row().gap(5.0).center_y().child(keycap(key, ink)).child(
        text(label.to_owned())
            .font(style(11.0, 500.0))
            .color(ink.tertiary),
    )
}

fn keycap(key: &str, ink: &Ink) -> El {
    row()
        .h(18.0)
        .px(6.0)
        .center()
        .radius(5.0)
        .fill(ink.selected)
        .border(0.5, ink.hairline)
        .child(
            text(key.to_owned())
                .font(style(10.5, 600.0))
                .color(ink.secondary),
        )
}

fn palette_row(index: usize, item: &PaletteItem, selected: bool, ink: &Ink) -> El {
    let action = item.action.clone();
    let mut r = row()
        .key(format!("palette-row-{index}"))
        .role("option")
        .label(item.title.clone())
        .hit()
        .w_full()
        .h(44.0)
        .px(10.0)
        .gap(12.0)
        .center_y()
        .radius(12.0)
        .hover(ink.hover)
        .on_press_tx(move |s: &mut App, tx| {
            s.close_palette(tx);
            s.run_palette_action(action.clone(), tx);
        })
        .child(
            row()
                .size(28.0, 28.0)
                .center()
                .radius(8.0)
                .fill(if selected { ink.accent } else { ink.selected })
                .child(
                    text(item.icon)
                        .font(style(12.0, 700.0).family(Family::Mono))
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
                    text(item.title.clone())
                        .font(style(13.5, 550.0))
                        .color(ink.text)
                        .truncate(),
                )
                .children(item.subtitle.clone().map(|s| {
                    text(s)
                        .font(style(11.5, 450.0))
                        .color(ink.secondary)
                        .truncate()
                })),
        )
        .child(pill(item.kind.clone(), ink.secondary, ink.hover));
    if let Some(shortcut) = item.shortcut {
        r = r.child(keycap(shortcut, ink));
    }
    if selected {
        r = r.fill(ink.accent.alpha(if ink.dark { 0.24 } else { 0.16 }));
    }
    r
}

/// The Touch ID context card: what sudo is about to run, from the kernel, in chrome program
/// output can't draw into. It never takes focus; the session announces it instead.
pub fn approval_card(c: &ApprovalContext, ink: &Ink) -> El {
    let waiting = c.phase == ApprovalPhase::WaitingForTouchId;
    let header = row()
        .w_full()
        .gap(10.0)
        .center_y()
        .child(
            row()
                .size(34.0, 34.0)
                .center()
                .radius(17.0)
                .fill(ink.accent.alpha(0.2))
                .border(1.5, ink.accent.alpha(0.7))
                .child(
                    text(if waiting { "◉" } else { "●" })
                        .font(style(16.0, 700.0))
                        .color(ink.accent),
                ),
        )
        .child(
            col()
                .gap(1.0)
                .child(
                    text(if waiting {
                        "Waiting for Touch ID…"
                    } else {
                        "Password requested"
                    })
                    .font(style(15.0, 650.0))
                    .color(ink.text),
                )
                .child(
                    text("sudo · approve on the sensor")
                        .font(style(11.5, 450.0))
                        .color(ink.secondary),
                ),
        );
    let from = [Some(c.tab_title.as_str()), c.directory.as_deref()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ");
    let grid = col()
        .w_full()
        .gap(6.0)
        .child(field("Command", &c.command, true, ink))
        .child(field("Run as", &c.run_as, false, ink))
        .child(field("Host", &c.host, false, ink))
        .child(field("From", &from, false, ink));
    let kernel = c.source == ApprovalSource::Kernel;
    let source = row()
        .gap(6.0)
        .center_y()
        .child(
            text(if kernel { "✓" } else { "⚠" })
                .font(style(11.0, 700.0))
                .color(if kernel { ink.green } else { ink.orange }),
        )
        .child(
            text(format!("Source: {}", c.source.as_str()))
                .font(style(11.5, 500.0))
                .color(if kernel { ink.secondary } else { ink.orange }),
        );
    let fallback = (!waiting).then(|| {
        text("Touch ID was cancelled or unavailable. Secure input is on for this tab.")
            .w_full()
            .wrap()
            .font(style(11.5, 450.0))
            .color(ink.secondary)
    });
    glass(
        col()
            .key("approval")
            .role("alert")
            .label(format!(
                "Approval request. sudo {}, run as {}, on {}. Source: {}.",
                c.command,
                c.run_as,
                c.host,
                c.source.as_str()
            ))
            .w(gh::size::CARD_WIDTH)
            .padding(gh::space::L)
            .gap(gh::space::M),
        ink,
        gh::radius::CARD,
        30.0,
        ink.glass_strong,
    )
    .shadow(16.0, 44.0, ink.shadow)
    .child(header)
    .child(el().w_full().h(0.5).fill(ink.hairline))
    .child(grid)
    .child(source)
    .children(fallback)
}

fn field(key: &str, value: &str, mono: bool, ink: &Ink) -> El {
    let value_style = if mono {
        style(12.5, 650.0).family(Family::Mono)
    } else {
        style(13.0, 450.0)
    };
    row()
        .w_full()
        .gap(gh::space::M)
        .child(
            text(key.to_owned())
                .font(style(12.5, 450.0))
                .color(ink.secondary)
                .w(64.0)
                .align(Align::End),
        )
        .child(
            text(value.to_owned())
                .font(value_style)
                .color(ink.text)
                .grow(1.0)
                .wrap()
                .max_lines(3),
        )
}

/// Find in the visible screen; matches are highlighted in the grid.
pub fn find_bar(app: &App, ink: &Ink) -> El {
    let Some(edit) = &app.find else { return el() };
    let matches = app.find_matches();
    let field = text_input(edit, style(13.0, 450.0).lh(20.0))
        .key("find-field")
        .single_line()
        .w(200.0)
        .color(ink.text)
        .caret(ink.accent, ink.accent.alpha(0.3))
        .placeholder("Find", ink.tertiary)
        .on_edit(|s: &mut App| match &mut s.find {
            Some(f) => f,
            None => &mut s.scratch,
        })
        .on_input_key(|s: &mut App, ev: &KeyEvent, tx: &mut Tx| {
            if ev.key == Key::Escape {
                s.find = None;
                tx.blur();
                return true;
            }
            false
        });
    let left = if app.show_sidebar {
        gh::size::SIDEBAR_WIDTH + super::MARGIN
    } else {
        0.0
    };
    let pane_right = super::MARGIN + gh::size::GUTTER_WIDTH + 24.0;
    let w = if app.window.0 > 0.0 {
        app.window.0
    } else {
        1180.0
    };
    let x = (w - pane_right - 300.0).max(left + 24.0);
    glass(
        row()
            .key("find")
            .absolute(x, super::TOOLBAR_HEIGHT + 14.0)
            .h(36.0)
            .pl(14.0)
            .pr(6.0)
            .gap(10.0)
            .center_y(),
        ink,
        18.0,
        24.0,
        ink.glass_strong,
    )
    .shadow(8.0, 24.0, ink.shadow)
    .child(text("⌕").font(style(14.0, 500.0)).color(ink.secondary))
    .child(field)
    .child(
        text(if edit.text.is_empty() {
            String::new()
        } else {
            format!("{matches} found")
        })
        .font(style(11.5, 500.0))
        .color(ink.secondary),
    )
    .child(
        row()
            .key("find-close")
            .role("button")
            .label("Close find bar")
            .hit()
            .size(24.0, 24.0)
            .center()
            .radius(12.0)
            .hover(ink.hover)
            .on_press_tx(|s: &mut App, tx| {
                s.find = None;
                tx.blur();
            })
            .child(text("×").font(style(14.0, 600.0)).color(ink.secondary)),
    )
}
