//! L1: the terminal pane — Rune's `grid` element fed from the engine's snapshot, the cursor,
//! find highlights, the command gutter and the approval card.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Mutex, OnceLock};

use design_system::gh;
use rune::tok::Family;
use rune::{Color, El, GridRow, Span, TextStyle, col, el, grid, row, text};
use shell_integration::CommandStatus;
use terminal_core::{CellFlags, GridLine, TerminalAppearance, TerminalCursorShape, TerminalEngine};

use design_system::TrustState;

use super::{Ink, UI, color, overlays, style};
use crate::app::{App, CellSize};
use crate::session::Session;
use crate::shortcuts::{self, Shortcut};

/// Font families are `&'static str` in Rune; each theme font is leaked once.
fn family(font: &str) -> Family {
    static NAMES: OnceLock<Mutex<HashMap<String, &'static str>>> = OnceLock::new();
    let names = NAMES.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut names) = names.lock() else {
        return Family::Mono;
    };
    let name = names
        .entry(font.to_owned())
        .or_insert_with(|| Box::leak(format!("{font}, {}", super::MONO_FAMILIES).into_boxed_str()));
    Family::Named(name)
}

/// The grid's text style: the theme font at its size, on a line height that is the cell height.
pub fn grid_style(a: &TerminalAppearance) -> TextStyle {
    let size = a.font_size.max(6.0);
    TextStyle::new(size, 400.0)
        .family(family(&a.font_name))
        .lh((size * 1.32).round())
}

/// One cell, measured from the font: the advance of ten zeros, so rounding can't drift a column.
pub fn measure_cell(a: &TerminalAppearance) -> CellSize {
    let style = grid_style(a);
    let width = rune::text::advance("0000000000", style) / 10.0;
    CellSize {
        width: if width > 0.0 { width } else { style.size * 0.6 },
        height: style.line_height,
    }
}

/// An invisible element that keeps the cell size current while no pane is on screen.
pub fn cell_probe(app: &App) -> El {
    let a = app
        .selected()
        .map_or_else(|| app.current_appearance(), |s| s.appearance.clone());
    el().key("cell-probe")
        .absolute(0.0, 0.0)
        .size(1.0, 1.0)
        .on_layout(move |s: &mut App, _| {
            s.set_cell(measure_cell(&a));
        })
}

pub fn pane(app: &App, ink: &Ink) -> El {
    let Some(session) = app.selected() else {
        return empty(ink);
    };
    let a = &session.appearance;
    let cell = app.cell.unwrap_or_else(|| measure_cell(a));

    let area = el()
        .key("grid-area")
        .grow(1.0)
        .h_full()
        .on_layout(|s: &mut App, r: [f32; 4]| s.set_pane_size(r[2], r[3]))
        .child(screen(app, session, cell, ink));

    let mut pane = col()
        .key(format!("pane-{}", session.id))
        .role("group")
        .label(session.title.clone())
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .child(header(session, ink))
        .child(super::hrule(ink))
        .child(
            row()
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .child(area)
                .child(gutter(session, ink, pane_height(app))),
        )
        .child(status_line(app, session, ink));
    if ink.glass {
        pane = pane.backdrop_blur(18.0);
    }
    if let Some(error) = &session.start_error {
        pane = pane.child(banner(&format!("the shell couldn't start: {error}"), ink));
    } else if session.exited {
        let code = session
            .exit_code
            .map_or("a signal".to_owned(), |c| format!("code {c}"));
        pane = pane.child(banner(
            &format!(
                "process exited with {code} · {} closes the tab",
                shortcuts::label(Shortcut::CloseTab)
            ),
            ink,
        ));
    }
    if let Some(context) = &session.approval {
        pane = pane.child(overlays::approval_card(context, ink).pin(
            None,
            None,
            Some(22.0),
            Some(STATUS_HEIGHT + 18.0),
        ));
    }
    if app.find.is_some() {
        pane = pane.child(overlays::find_bar(app, ink).pin(
            None,
            Some(HEADER_HEIGHT + 10.0),
            Some(28.0),
            None,
        ));
    }
    pane
}

const HEADER_HEIGHT: f32 = 28.0;
const STATUS_HEIGHT: f32 = 24.0;

/// `⠿   running command   duration`: the command the tab is running, or last ran.
fn header(session: &Session, ink: &Ink) -> El {
    let record = session.timeline.commands().iter().rev().find(|r| {
        r.command_line
            .as_deref()
            .is_some_and(|c| !c.trim().is_empty())
    });
    let command = record
        .and_then(|r| r.command_line.as_deref())
        .map(strip_prompt);
    let (title, running) = match (&command, record.map(|r| r.status())) {
        (Some(c), Some(CommandStatus::Running)) => (c.clone(), true),
        (Some(c), _) => (c.clone(), false),
        _ => (session.title.clone(), false),
    };
    let duration = record
        .and_then(|r| r.duration_at(std::time::SystemTime::now()))
        .map(format_duration);
    row()
        .key("pane-header")
        .w_full()
        .h(HEADER_HEIGHT)
        .px(10.0)
        .gap(10.0)
        .center_y()
        .fill(ink.surface(ink.panel))
        .child(text("⠿").font(style(UI, 600.0)).color(ink.tertiary))
        .child(el().grow(1.0))
        .child(
            text(title)
                .font(style(UI, 650.0))
                .color(if running { ink.text } else { ink.secondary })
                .truncate()
                .max_w(560.0),
        )
        .child(el().grow(1.0))
        .children(duration.map(|d| text(d).font(style(UI, 500.0)).color(ink.tertiary)))
}

/// The typed command from a captured line that starts with the prompt (display only).
pub fn strip_prompt(line: &str) -> String {
    let line = line.lines().next().unwrap_or(line);
    ["❯ ", "$ ", "% ", "# ", "> "]
        .iter()
        .filter_map(|marker| line.rfind(marker).map(|i| i + marker.len()))
        .max()
        .map_or(line, |i| &line[i..])
        .trim()
        .to_owned()
}

/// `3m32s`, `1.8s`, `40ms`.
pub fn format_duration(d: std::time::Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 {
        format!("{}h{:02}m", s / 3600, (s % 3600) / 60)
    } else if s >= 60 {
        format!("{}m{:02}s", s / 60, s % 60)
    } else if s >= 1 {
        format!("{:.1}s", d.as_secs_f64())
    } else {
        format!("{}ms", d.as_millis())
    }
}

/// A vim-style status line: an inverted mode block, the directory, then the grid's facts.
fn status_line(app: &App, session: &Session, ink: &Ink) -> El {
    let (mode, fill) = if session.exited {
        ("EXITED".to_owned(), ink.tertiary)
    } else {
        match &session.trust {
            TrustState::Idle => ("SHELL".to_owned(), ink.accent),
            TrustState::SecureInput => ("SECRET".to_owned(), ink.blue),
            TrustState::TouchIdSudo => ("SUDO".to_owned(), ink.blue),
            TrustState::RemoteSession(_) => ("SSH".to_owned(), ink.orange),
        }
    };
    let place = match &session.trust {
        TrustState::RemoteSession(host) => host.clone(),
        _ => session
            .cwd
            .as_deref()
            .map(super::home_relative)
            .unwrap_or_else(|| session.title.clone()),
    };
    let size = session.engine().size();
    let last = session
        .timeline
        .commands()
        .iter()
        .rev()
        .find(|r| r.finished_at.is_some());
    let exit = last.and_then(|r| r.exit_code).map(|code| {
        let (glyph, c) = if code == 0 {
            ("✓", ink.green)
        } else {
            ("✗", ink.red)
        };
        text(format!("{glyph} {code}"))
            .font(style(UI, 600.0))
            .color(c)
    });
    let commands = session.timeline.commands().len();
    let shell = std::path::Path::new(&std::env::var("SHELL").unwrap_or_else(|_| "sh".into()))
        .file_name()
        .map_or_else(|| "sh".to_owned(), |n| n.to_string_lossy().into_owned());
    let kind = if matches!(session.trust, TrustState::RemoteSession(_)) {
        "ssh".to_owned()
    } else {
        shell
    };
    let fact = |s: String| text(s).font(style(UI, 500.0)).color(ink.secondary);
    let _ = app;
    row()
        .key("status-line")
        .w_full()
        .h(STATUS_HEIGHT)
        .gap(12.0)
        .center_y()
        .fill(ink.surface(ink.panel))
        .child(
            row()
                .h_full()
                .px(10.0)
                .center_y()
                .fill(fill)
                .child(text(mode).font(style(UI, 750.0)).color(ink.on_accent)),
        )
        .child(
            text(place)
                .font(style(UI, 500.0))
                .color(ink.text)
                .truncate_head()
                .max_w(420.0),
        )
        .child(el().grow(1.0))
        .children(exit)
        .child(fact(format!(
            "{commands} cmd{}",
            if commands == 1 { "" } else { "s" }
        )))
        .child(fact(format!("{}×{}", size.cols, size.rows)))
        .child(
            row()
                .h_full()
                .px(10.0)
                .center_y()
                .fill(fill.alpha(0.85))
                .child(text(kind).font(style(UI, 700.0)).color(ink.on_accent)),
        )
}

/// The pane's grid height from layout, or from the window before the first layout.
fn pane_height(app: &App) -> f32 {
    if app.pane_size.1 > 0.0 {
        app.pane_size.1
    } else {
        let window = if app.window.1 > 0.0 {
            app.window.1
        } else {
            crate::SIZE.1
        };
        window - super::TITLE_HEIGHT - super::BAR_HEIGHT - HEADER_HEIGHT - STATUS_HEIGHT - 4.0
    }
}

fn empty(ink: &Ink) -> El {
    col()
        .key("empty")
        .grow(1.0)
        .h_full()
        .center()
        .gap(6.0)
        .child(text("no tabs").font(style(15.0, 650.0)).color(ink.text))
        .child(
            text(format!("{} opens one", shortcuts::label(Shortcut::NewTab)))
                .font(style(UI, 450.0))
                .color(ink.tertiary),
        )
}

fn banner(message: &str, ink: &Ink) -> El {
    super::panel(
        row()
            .key("banner")
            .pin(Some(0.0), None, Some(0.0), Some(STATUS_HEIGHT))
            .h(26.0)
            .px(12.0)
            .gap(8.0)
            .center_y(),
        ink,
        ink.panel,
    )
    .child(text("!").font(style(UI, 800.0)).color(ink.orange))
    .child(
        text(message.to_owned())
            .font(style(UI, 500.0))
            .color(ink.text)
            .truncate(),
    )
}

/// The visible rows as one `grid`, with the cursor over it.
fn screen(app: &App, session: &Session, cell: CellSize, ink: &Ink) -> El {
    let a = &session.appearance;
    let snap = session.engine().snapshot();
    let style = grid_style(a);
    let find = app
        .find
        .as_ref()
        .map(|f| f.text.to_lowercase())
        .filter(|q| !q.is_empty());
    let highlight = ink.orange.alpha(0.42);
    let rows: Vec<Rc<GridRow>> = snap
        .lines
        .iter()
        .map(|line| Rc::new(grid_row(line, style, find.as_deref(), highlight)))
        .collect();
    let cols = u16::try_from(snap.columns).unwrap_or(u16::MAX);
    let (pad_x, pad_y) = (a.padding_horizontal, a.padding_vertical);
    let height = cell.height * snap.lines.len() as f32;
    let top = snap.history_size.saturating_sub(snap.display_offset);

    let mut screen = grid(cols, cell.width, cell.height, rows)
        .key("grid")
        .absolute(pad_x, pad_y)
        .w(cell.width * f32::from(cols))
        .h(height);
    let cursor = &snap.cursor;
    let cursor_row = cursor
        .row
        .checked_sub(top)
        .filter(|r| *r < snap.screen_lines);
    if let Some(r) = cursor_row
        && cursor.visible
    {
        screen = screen.grid_caret(cursor.col as u16, r as u16);
    }
    let mut layer = el().w_full().h_full().child(screen);
    if let Some(r) = cursor_row
        && cursor.visible
        && (app.blink_on || !cursor.blinking)
    {
        let x = pad_x + cursor.col as f32 * cell.width;
        let y = pad_y + r as f32 * cell.height;
        let under = snap
            .lines
            .get(r)
            .and_then(|l| l.cells.get(cursor.col))
            .map_or(' ', |c| c.ch);
        let fill = color(cursor.color);
        let mark = match cursor.shape {
            TerminalCursorShape::Block => {
                let ink = cursor.text_color.map_or(color(snap.background), color);
                el().absolute(x, y)
                    .size(cell.width, cell.height)
                    .radius(1.5)
                    .fill(fill)
                    .child(
                        text(under.to_string())
                            .font(style)
                            .color(ink)
                            .absolute(0.0, 0.0),
                    )
            }
            TerminalCursorShape::Underline => el()
                .absolute(x, y + cell.height - 2.0)
                .size(cell.width, 2.0)
                .fill(fill),
            TerminalCursorShape::Bar => el().absolute(x, y).size(2.0, cell.height).fill(fill),
        };
        layer = layer.child(mark.key("cursor"));
    }
    layer
}

/// A grid line as Rune's row: narrow characters in the shaped line, wide and non-ASCII
/// glyphs pinned to their columns, and non-default backgrounds as fills.
fn grid_row(line: &GridLine, style: TextStyle, find: Option<&str>, highlight: Color) -> GridRow {
    let mut content = String::with_capacity(line.cells.len());
    let mut spans: Vec<Span> = Vec::new();
    let mut pinned = Vec::new();
    let mut fills: Vec<(u16, u16, Color)> = Vec::new();
    let mut run: Option<(usize, terminal_core::GridCell)> = None;

    let close =
        |spans: &mut Vec<Span>, run: &Option<(usize, terminal_core::GridCell)>, end: usize| {
            if let Some((start, cell)) = run
                && end > *start
            {
                let mut span = Span::new(*start, end).color(color(cell.fg));
                if cell.flags.contains(CellFlags::BOLD) {
                    span = span.bold();
                }
                if cell.flags.contains(CellFlags::ITALIC) {
                    span = span.italic();
                }
                if cell.flags.contains(CellFlags::UNDERLINE)
                    || cell.flags.contains(CellFlags::DOUBLE_UNDERLINE)
                {
                    span.underline = Some(color(cell.fg));
                }
                spans.push(span);
            }
        };

    for (col, cell) in line.cells.iter().enumerate() {
        let col16 = u16::try_from(col).unwrap_or(u16::MAX);
        if !cell.flags.contains(CellFlags::DEFAULT_BACKGROUND)
            && !cell.flags.contains(CellFlags::WIDE_SPACER)
        {
            let bg = color(cell.bg);
            let width = if cell.flags.contains(CellFlags::WIDE) {
                2
            } else {
                1
            };
            match fills.last_mut() {
                Some((_, end, c)) if *end == col16 && *c == bg => *end += width,
                _ => fills.push((col16, col16 + width, bg)),
            }
        }
        if cell.flags.contains(CellFlags::WIDE_SPACER) {
            content.push(' ');
            continue;
        }
        let narrow_ascii = cell.ch.is_ascii()
            && cell.zerowidth.is_empty()
            && !cell.flags.contains(CellFlags::WIDE);
        if !narrow_ascii && cell.ch != ' ' {
            let width = if cell.flags.contains(CellFlags::WIDE) {
                2
            } else {
                1
            };
            let mut glyph = cell.ch.to_string();
            glyph.extend(&cell.zerowidth);
            let mut s = style;
            if cell.flags.contains(CellFlags::BOLD) {
                s.weight = 700.0;
            }
            pinned.push((col16, width, glyph, s, color(cell.fg)));
            content.push(' ');
            continue;
        }
        let same = run
            .as_ref()
            .is_some_and(|(_, r)| r.fg == cell.fg && r.flags == cell.flags);
        if !same {
            close(&mut spans, &run, content.len());
            run = Some((content.len(), cell.clone()));
        }
        content.push(if cell.flags.contains(CellFlags::HIDDEN) {
            ' '
        } else {
            cell.ch
        });
    }
    close(&mut spans, &run, content.len());

    let mut row = GridRow::new(content, spans, style);
    if let Some(query) = find {
        let text: Vec<char> = line
            .cells
            .iter()
            .map(|c| c.ch.to_ascii_lowercase())
            .collect();
        let q: Vec<char> = query.chars().collect();
        let mut i = 0;
        while !q.is_empty() && i + q.len() <= text.len() {
            if text[i..i + q.len()] == q[..] {
                fills.push((i as u16, (i + q.len()) as u16, highlight));
                i += q.len();
            } else {
                i += 1;
            }
        }
    }
    for (start, end, c) in fills {
        row = row.fill(start, end, c);
    }
    for (col, width, glyph, s, c) in pinned {
        row = row.cell(col, width, glyph, s, c);
    }
    row
}

/// Command marks in a reserved strip right of the grid. Shape and colour both encode status
/// (never colour alone): ● success, ■ failure, ○ running.
fn gutter(session: &Session, ink: &Ink, height: f32) -> El {
    let total = session.engine().total_rows().max(1) as f32;
    let ticks = session.timeline.commands().iter().map(|r| {
        let hit = gh::size::GUTTER_HIT_HEIGHT;
        let y =
            (r.prompt_row as f32 / total * height - hit / 2.0).clamp(0.0, (height - hit).max(0.0));
        let s = gh::size::GUTTER_TICK;
        let label = r.command_line.clone().unwrap_or_else(|| "command".into());
        let mark = match r.status() {
            CommandStatus::Succeeded => el().size(s, s).radius(s / 2.0).fill(ink.secondary),
            CommandStatus::Failed => el().size(s, s).fill(ink.red),
            CommandStatus::Running => el().size(s, s).radius(s / 2.0).border(1.5, ink.blue),
            CommandStatus::Unknown => el().size(s - 2.0, 2.0).radius(1.0).fill(ink.tertiary),
        };
        let record = r.clone();
        let help = match (r.status(), r.exit_code) {
            (CommandStatus::Failed, Some(code)) => format!("{label} — exit {code}"),
            (CommandStatus::Running, _) => format!("{label} — running"),
            _ => label.clone(),
        };
        row()
            .role("button")
            .label(help.clone())
            .help(help)
            .hit()
            .absolute(0.0, y)
            .w(gh::size::GUTTER_WIDTH)
            .h(gh::size::GUTTER_HIT_HEIGHT)
            .center()
            .on_press(move |s: &mut App| {
                if let Some(session) = s.selected() {
                    session.jump_to(&record);
                }
            })
            .child(mark)
    });
    el().key("gutter")
        .role("group")
        .label("Commands")
        .w(gh::size::GUTTER_WIDTH + 4.0)
        .h_full()
        .children(ticks)
}
