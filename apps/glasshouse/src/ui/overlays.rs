//! L3: transient boxes — the command palette, the approval card and the find bar — each titled
//! in its top rule, and the dimming behind a modal one.

use design_system::gh;
use rune::{Align, Color, El, Key, KeyEvent, Tx, col, el, row, text, text_input};

use super::{Ink, UI, hrule, style, titled};
use crate::app::App;
use crate::palette::PaletteItem;
use crate::session::{ApprovalContext, ApprovalPhase, ApprovalSource};

/// Dims everything behind a modal overlay; pressing it dismisses the overlay.
pub fn dim(key: &str, ink: &Ink, dismiss: fn(&mut App, &mut Tx)) -> El {
    el().key(key)
        .absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .hit()
        .fill(Color::BLACK.alpha(if ink.dark { 0.55 } else { 0.35 }))
        .on_press_tx(move |s: &mut App, tx| dismiss(s, tx))
}

const PALETTE_ROWS: usize = 12;
const PALETTE_WIDTH: f32 = 680.0;

pub fn palette(app: &App, ink: &Ink) -> El {
    let Some(state) = &app.palette else {
        return el();
    };
    let results = app.palette_results();
    let selection = state.selection.min(results.len().saturating_sub(1));
    let first = selection.saturating_sub(PALETTE_ROWS - 1);

    let field = text_input(&state.edit, style(14.0, 500.0).lh(22.0))
        .key("palette-field")
        .single_line()
        .grow(1.0)
        .color(ink.text)
        .caret(ink.accent, ink.accent.alpha(0.3))
        .placeholder("actions, hosts, tabs, recent commands", ink.tertiary)
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

    let count = results.len();
    let rows = results
        .iter()
        .enumerate()
        .skip(first)
        .take(PALETTE_ROWS)
        .map(|(i, item)| palette_row(i, item, i == selection, ink));
    let mut list = col().key("palette-results").w_full().py(4.0).children(rows);
    if results.is_empty() {
        list = list.child(
            text("no matches")
                .font(style(UI, 450.0))
                .color(ink.tertiary)
                .px(14.0)
                .py(8.0),
        );
    }
    let w = if app.window.0 > 0.0 {
        app.window.0
    } else {
        crate::SIZE.0
    };
    let footer = row()
        .w_full()
        .h(24.0)
        .px(14.0)
        .gap(16.0)
        .center_y()
        .child(hint("↑↓", "move", ink))
        .child(hint("↩", "run", ink))
        .child(hint("esc", "close", ink))
        .child(el().grow(1.0))
        .child(
            text(format!(
                "{}/{count}",
                if count == 0 { 0 } else { selection + 1 }
            ))
            .font(style(11.5, 500.0))
            .color(ink.tertiary),
        );
    titled(
        col()
            .key("palette")
            .absolute(((w - PALETTE_WIDTH) / 2.0).max(12.0), 72.0)
            .w(PALETTE_WIDTH),
        "command palette",
        ink,
    )
    .child(
        row()
            .w_full()
            .h(44.0)
            .px(14.0)
            .gap(10.0)
            .center_y()
            .child(text("❯").font(style(15.0, 700.0)).color(ink.accent))
            .child(field),
    )
    .child(hrule(ink))
    .child(list)
    .child(hrule(ink))
    .child(footer)
}

fn hint(key: &str, label: &str, ink: &Ink) -> El {
    row()
        .gap(6.0)
        .center_y()
        .child(
            text(key.to_owned())
                .font(style(11.5, 650.0))
                .color(ink.secondary),
        )
        .child(
            text(label.to_owned())
                .font(style(11.5, 450.0))
                .color(ink.tertiary),
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
        .h(26.0)
        .px(10.0)
        .gap(10.0)
        .center_y()
        .hover(ink.hover)
        .on_press_tx(move |s: &mut App, tx| {
            s.close_palette(tx);
            s.run_palette_action(action.clone(), tx);
        })
        .child(
            text(if selected { "▸" } else { " " })
                .font(style(UI, 700.0))
                .color(ink.accent),
        )
        .child(
            text(item.icon)
                .font(style(UI, 600.0))
                .color(if selected { ink.accent } else { ink.tertiary })
                .w(18.0),
        )
        .child(
            text(item.title.clone())
                .font(style(UI, if selected { 650.0 } else { 500.0 }))
                .color(ink.text)
                .truncate()
                .max_w(300.0),
        )
        .children(item.subtitle.clone().map(|s| {
            text(s)
                .font(style(11.5, 450.0))
                .color(ink.tertiary)
                .truncate()
        }))
        .child(el().grow(1.0))
        .child(
            text(item.kind.to_lowercase())
                .font(style(11.5, 500.0))
                .color(ink.secondary),
        );
    if let Some(shortcut) = item.shortcut {
        r = r.child(
            text(shortcut)
                .font(style(11.5, 650.0))
                .color(ink.secondary)
                .w(110.0)
                .align(Align::End),
        );
    } else {
        r = r.child(el().w(110.0));
    }
    if selected {
        r = r.fill(ink.selected);
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
            text(if waiting { "◉" } else { "●" })
                .font(style(15.0, 700.0))
                .color(ink.blue),
        )
        .child(
            text(if waiting {
                "waiting for touch id…"
            } else {
                "password requested"
            })
            .font(style(13.5, 700.0))
            .color(ink.text),
        )
        .child(el().grow(1.0))
        .child(text("sudo").font(style(UI, 600.0)).color(ink.tertiary));
    let from = [Some(c.tab_title.as_str()), c.directory.as_deref()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ");
    let kernel = c.source == ApprovalSource::Kernel;
    let source = row()
        .gap(8.0)
        .center_y()
        .child(
            text(if kernel { "✓" } else { "!" })
                .font(style(UI, 700.0))
                .color(if kernel { ink.green } else { ink.orange }),
        )
        .child(
            text(format!("source: {}", c.source.as_str()))
                .font(style(11.5, 500.0))
                .color(if kernel { ink.secondary } else { ink.orange }),
        );
    let fallback = (!waiting).then(|| {
        text("Touch ID was cancelled or unavailable. sudo is asking for your password in this tab.")
            .w_full()
            .wrap()
            .font(style(11.5, 450.0))
            .color(ink.secondary)
    });
    titled(
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
            .w(gh::size::CARD_WIDTH + 40.0)
            .px(16.0)
            .pt(18.0)
            .pb(14.0)
            .gap(10.0),
        "touch id",
        ink,
    )
    .child(header)
    .child(hrule(ink))
    .child(
        col()
            .w_full()
            .gap(4.0)
            .child(field("command", &c.command, true, ink))
            .child(field("run as", &c.run_as, false, ink))
            .child(field("host", &c.host, false, ink))
            .child(field("from", &from, false, ink)),
    )
    .child(source)
    .children(fallback)
}

fn field(key: &str, value: &str, strong: bool, ink: &Ink) -> El {
    row()
        .w_full()
        .gap(12.0)
        .child(
            text(key.to_owned())
                .font(style(UI, 450.0))
                .color(ink.tertiary)
                .w(64.0),
        )
        .child(
            text(value.to_owned())
                .font(style(UI, if strong { 700.0 } else { 500.0 }))
                .color(if strong { ink.orange } else { ink.text })
                .grow(1.0)
                .min_w(0.0)
                .wrap()
                .max_lines(3),
        )
}

/// Find in the visible screen, vim-style: `/query   N matches`. Matches are highlighted in the grid.
pub fn find_bar(app: &App, ink: &Ink) -> El {
    let Some(edit) = &app.find else { return el() };
    let matches = app.find_matches();
    let field = text_input(edit, style(UI, 500.0).lh(18.0))
        .key("find-field")
        .single_line()
        .w(220.0)
        .color(ink.text)
        .caret(ink.accent, ink.accent.alpha(0.3))
        .placeholder("find", ink.tertiary)
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
    super::panel(
        row()
            .key("find")
            .h(30.0)
            .pl(12.0)
            .pr(4.0)
            .gap(8.0)
            .center_y(),
        ink,
        ink.panel,
    )
    .shadow(8.0, 24.0, ink.shadow)
    .child(text("/").font(style(13.0, 700.0)).color(ink.accent))
    .child(field)
    .child(
        text(if edit.text.is_empty() {
            String::new()
        } else {
            format!("{matches} match{}", if matches == 1 { "" } else { "es" })
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
            .size(24.0, 22.0)
            .center()
            .hover(ink.selected)
            .on_press_tx(|s: &mut App, tx| {
                s.find = None;
                tx.blur();
            })
            .child(text("esc").font(style(11.0, 600.0)).color(ink.tertiary)),
    )
}
