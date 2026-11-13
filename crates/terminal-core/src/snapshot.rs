use std::ops::Range;

use alacritty_terminal::Term;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::TermMode;
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor, Rgb};
use design_system::Rgba;

use crate::TerminalCursorShape;
use crate::appearance::{TerminalAppearance, xterm_256};

/// Style flags of a [`GridCell`]. Colors in the cell are already resolved (inverse/hidden/dim/bold-bright applied).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct CellFlags(u16);

impl CellFlags {
    /// Bold weight.
    pub const BOLD: Self = Self(1);
    /// Italic.
    pub const ITALIC: Self = Self(1 << 1);
    /// Single (or any other style of) underline.
    pub const UNDERLINE: Self = Self(1 << 2);
    /// Double underline.
    pub const DOUBLE_UNDERLINE: Self = Self(1 << 3);
    /// Curly underline.
    pub const UNDERCURL: Self = Self(1 << 4);
    /// Strikethrough.
    pub const STRIKEOUT: Self = Self(1 << 5);
    /// SGR 7 reverse video (already applied to `fg`/`bg`).
    pub const INVERSE: Self = Self(1 << 6);
    /// SGR 2 faint (already applied to `fg`).
    pub const DIM: Self = Self(1 << 7);
    /// SGR 8 concealed (`fg` already equals `bg`).
    pub const HIDDEN: Self = Self(1 << 8);
    /// A double-width character; the next cell is a [`WIDE_SPACER`](Self::WIDE_SPACER).
    pub const WIDE: Self = Self(1 << 9);
    /// Placeholder after a wide character; draw nothing.
    pub const WIDE_SPACER: Self = Self(1 << 10);
    /// The background is the terminal's default (the UI may skip filling it, honoring background opacity).
    pub const DEFAULT_BACKGROUND: Self = Self(1 << 11);

    /// No flags.
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Whether every flag in `other` is set.
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Raw bits.
    pub const fn bits(self) -> u16 {
        self.0
    }

    fn set(&mut self, flag: Self, on: bool) {
        if on {
            self.0 |= flag.0;
        }
    }

    /// Flags relevant to text style (excludes the wide-character layout flags).
    fn style(self) -> Self {
        Self(self.0 & !(Self::WIDE.0 | Self::WIDE_SPACER.0))
    }
}

impl std::ops::BitOr for CellFlags {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

/// One cell with resolved colors.
#[derive(Debug, Clone, PartialEq)]
pub struct GridCell {
    /// The character (`' '` for empty cells).
    pub ch: char,
    /// Combining characters drawn on top of `ch`.
    pub zerowidth: Vec<char>,
    /// Text color.
    pub fg: Rgba,
    /// Background color.
    pub bg: Rgba,
    /// Style flags.
    pub flags: CellFlags,
}

/// A run of cells with identical style, for span-based text rendering.
#[derive(Debug, Clone, PartialEq)]
pub struct CellSpan {
    /// First column of the run.
    pub start_col: usize,
    /// Width in cells (wide characters count twice).
    pub width: usize,
    /// Text of the run (wide-character spacers omitted).
    pub text: String,
    /// Text color.
    pub fg: Rgba,
    /// Background color.
    pub bg: Rgba,
    /// Style flags (without the wide-character layout flags).
    pub flags: CellFlags,
}

/// One grid line.
#[derive(Debug, Clone, PartialEq)]
pub struct GridLine {
    /// Absolute row.
    pub row: usize,
    /// Exactly `columns` cells.
    pub cells: Vec<GridCell>,
    /// The line soft-wraps into the next one.
    pub wrapped: bool,
}

impl GridLine {
    /// Plain text of the line with trailing whitespace trimmed.
    pub fn text(&self) -> String {
        let mut s = String::with_capacity(self.cells.len());
        for cell in self
            .cells
            .iter()
            .filter(|c| !c.flags.contains(CellFlags::WIDE_SPACER))
        {
            s.push(cell.ch);
            s.extend(&cell.zerowidth);
        }
        s.truncate(s.trim_end().len());
        s
    }

    /// Groups the line into runs of equal style.
    pub fn spans(&self) -> Vec<CellSpan> {
        let mut spans: Vec<CellSpan> = Vec::new();
        for (col, cell) in self.cells.iter().enumerate() {
            let style = cell.flags.style();
            let spacer = cell.flags.contains(CellFlags::WIDE_SPACER);
            let span = match spans.last_mut() {
                Some(s) if spacer || (s.fg == cell.fg && s.bg == cell.bg && s.flags == style) => s,
                _ => {
                    spans.push(CellSpan {
                        start_col: col,
                        width: 0,
                        text: String::new(),
                        fg: cell.fg,
                        bg: cell.bg,
                        flags: style,
                    });
                    let Some(s) = spans.last_mut() else { continue };
                    s
                }
            };
            span.width += 1;
            if !spacer {
                span.text.push(cell.ch);
                span.text.extend(&cell.zerowidth);
            }
        }
        spans
    }
}

/// Cursor state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CursorSnapshot {
    /// Absolute row.
    pub row: usize,
    /// Column.
    pub col: usize,
    /// Shown (DECTCEM) and not hidden by style.
    pub visible: bool,
    /// Current shape (the appearance default unless a program changed it).
    pub shape: TerminalCursorShape,
    /// Whether it blinks.
    pub blinking: bool,
    /// Cursor color.
    pub color: Rgba,
    /// Text color under a block cursor, if configured.
    pub text_color: Option<Rgba>,
}

/// Terminal modes the UI needs for input handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TerminalModes {
    /// DECCKM: application cursor keys (pass to [`encode_key`](crate::encode_key)).
    pub app_cursor: bool,
    /// DECKPAM: application keypad.
    pub app_keypad: bool,
    /// DECSET 2004: wrap pastes (pass to [`encode_paste`](crate::encode_paste)).
    pub bracketed_paste: bool,
    /// The alternate screen is active (full-screen app).
    pub alt_screen: bool,
    /// DECTCEM.
    pub show_cursor: bool,
    /// A program asked for mouse reports.
    pub mouse_reporting: bool,
    /// Mouse reports use SGR encoding.
    pub sgr_mouse: bool,
    /// A program asked for focus in/out reports.
    pub focus_reporting: bool,
}

impl TerminalModes {
    pub(crate) fn from_term(mode: TermMode) -> Self {
        Self {
            app_cursor: mode.contains(TermMode::APP_CURSOR),
            app_keypad: mode.contains(TermMode::APP_KEYPAD),
            bracketed_paste: mode.contains(TermMode::BRACKETED_PASTE),
            alt_screen: mode.contains(TermMode::ALT_SCREEN),
            show_cursor: mode.contains(TermMode::SHOW_CURSOR),
            mouse_reporting: mode.intersects(TermMode::MOUSE_MODE),
            sgr_mouse: mode.contains(TermMode::SGR_MOUSE),
            focus_reporting: mode.contains(TermMode::FOCUS_IN_OUT),
        }
    }
}

/// A copy of part of the grid, safe to render without holding any engine lock.
#[derive(Debug, Clone, PartialEq)]
pub struct GridSnapshot {
    /// Grid width.
    pub columns: usize,
    /// Visible rows.
    pub screen_lines: usize,
    /// Scrollback lines above the screen.
    pub history_size: usize,
    /// How many lines the viewport is scrolled back.
    pub display_offset: usize,
    /// The requested lines, in order (absolute rows outside the buffer are omitted).
    pub lines: Vec<GridLine>,
    /// Cursor (its row may be outside `lines`).
    pub cursor: CursorSnapshot,
    /// Effective default foreground (including OSC 10 overrides).
    pub foreground: Rgba,
    /// Effective default background (including OSC 11 overrides); draw with `background_opacity`.
    pub background: Rgba,
    /// Input-relevant modes.
    pub modes: TerminalModes,
}

pub(crate) fn absolute_row<T>(term: &Term<T>, line: Line) -> usize {
    usize::try_from(term.grid().history_size() as i64 + i64::from(line.0)).unwrap_or(0)
}

pub(crate) fn grid_line<T>(term: &Term<T>, row: usize) -> Option<Line> {
    let grid = term.grid();
    if row >= grid.total_lines() {
        return None;
    }
    let line = i64::try_from(row).ok()? - grid.history_size() as i64;
    i32::try_from(line).ok().map(Line)
}

pub(crate) fn top_visible_row<T>(term: &Term<T>) -> usize {
    term.grid()
        .history_size()
        .saturating_sub(term.grid().display_offset())
}

pub(crate) fn build<T>(
    term: &Term<T>,
    rows: Range<usize>,
    appearance: &TerminalAppearance,
) -> GridSnapshot {
    let grid = term.grid();
    let colors = term.colors();
    let palette = Palette { colors, appearance };
    let lines = rows
        .filter_map(|row| grid_line(term, row).map(|line| (row, line)))
        .map(|(row, line)| {
            let grid_row = &grid[line];
            let cells: Vec<GridCell> = (0..grid.columns())
                .map(|col| palette.cell(&grid_row[Column(col)]))
                .collect();
            let wrapped = grid.columns() > 0
                && grid_row[Column(grid.columns() - 1)]
                    .flags
                    .contains(Flags::WRAPLINE);
            GridLine {
                row,
                cells,
                wrapped,
            }
        })
        .collect();

    let mode = *term.mode();
    let style = term.cursor_style();
    let point = grid.cursor.point;
    let shape = match style.shape {
        CursorShape::Underline => TerminalCursorShape::Underline,
        CursorShape::Beam => TerminalCursorShape::Bar,
        CursorShape::Block | CursorShape::HollowBlock | CursorShape::Hidden => {
            TerminalCursorShape::Block
        }
    };
    let cursor = CursorSnapshot {
        row: absolute_row(term, point.line),
        col: point.column.0,
        visible: mode.contains(TermMode::SHOW_CURSOR) && style.shape != CursorShape::Hidden,
        shape,
        blinking: style.blinking,
        color: palette.index(NamedColor::Cursor as usize),
        text_color: appearance.cursor_text,
    };

    GridSnapshot {
        columns: grid.columns(),
        screen_lines: grid.screen_lines(),
        history_size: grid.history_size(),
        display_offset: grid.display_offset(),
        lines,
        cursor,
        foreground: palette.index(NamedColor::Foreground as usize),
        background: palette.index(NamedColor::Background as usize),
        modes: TerminalModes::from_term(mode),
    }
}

/// Plain text of absolute rows `[from, to)`.
pub(crate) fn text<T>(term: &Term<T>, from: usize, to: usize) -> String {
    let grid = term.grid();
    let mut out: Vec<String> = Vec::new();
    for row in from..to {
        let Some(line) = grid_line(term, row) else {
            continue;
        };
        let grid_row = &grid[line];
        let mut s = String::with_capacity(grid.columns());
        for col in 0..grid.columns() {
            let cell = &grid_row[Column(col)];
            if cell
                .flags
                .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
            {
                continue;
            }
            s.push(cell.c);
            if let Some(zw) = cell.zerowidth() {
                s.extend(zw);
            }
        }
        s.truncate(s.trim_end().len());
        out.push(s);
    }
    out.join("\n")
}

/// Resolves alacritty colors: terminal overrides (OSC 4/10/11/12) first, then the appearance.
pub(crate) struct Palette<'a> {
    pub(crate) colors: &'a Colors,
    pub(crate) appearance: &'a TerminalAppearance,
}

fn rgb(c: Rgb) -> Rgba {
    Rgba::from_rgb8(c.r, c.g, c.b)
}

fn dim(c: Rgba) -> Rgba {
    Rgba::new(c.r * 2.0 / 3.0, c.g * 2.0 / 3.0, c.b * 2.0 / 3.0, c.a)
}

const DIM_BLACK: usize = NamedColor::DimBlack as usize;
const DIM_WHITE: usize = NamedColor::DimWhite as usize;

impl Palette<'_> {
    /// Color for an alacritty color index (0…268, see `alacritty_terminal::term::color`).
    pub(crate) fn index(&self, i: usize) -> Rgba {
        if let Some(Some(c)) = (i < alacritty_terminal::term::color::COUNT).then(|| self.colors[i])
        {
            return rgb(c);
        }
        let a = self.appearance;
        match i {
            0..16 => a.ansi[i],
            16..256 => xterm_256(u8::try_from(i).unwrap_or(u8::MAX)),
            _ if i == NamedColor::Background as usize => a.background,
            _ if i == NamedColor::Cursor as usize => a.cursor,
            DIM_BLACK..=DIM_WHITE => dim(self.index(i - DIM_BLACK)),
            _ if i == NamedColor::DimForeground as usize => {
                dim(self.index(NamedColor::Foreground as usize))
            }
            // Foreground, BrightForeground and anything unknown.
            _ if i == NamedColor::BrightForeground as usize => {
                self.index(NamedColor::Foreground as usize)
            }
            _ => a.foreground,
        }
    }

    fn color(&self, color: Color) -> Rgba {
        match color {
            Color::Spec(c) => rgb(c),
            Color::Indexed(i) => self.index(usize::from(i)),
            Color::Named(n) => self.index(n as usize),
        }
    }

    fn cell(&self, cell: &Cell) -> GridCell {
        let f = cell.flags;
        let bold = f.contains(Flags::BOLD);
        let brighten = bold && self.appearance.bold_is_bright;
        let fg_color = match cell.fg {
            Color::Named(n) if brighten && (n as usize) < 8 => Color::Indexed(n as u8 + 8),
            Color::Named(NamedColor::Foreground) if brighten => {
                Color::Named(NamedColor::BrightForeground)
            }
            Color::Indexed(i) if brighten && i < 8 => Color::Indexed(i + 8),
            other => other,
        };
        // A theme's bold colour replaces the default foreground for bold text, as in iTerm2.
        let default_bold = match self.appearance.bold {
            Some(bold_color) if bold && cell.fg == Color::Named(NamedColor::Foreground) => {
                Some(bold_color)
            }
            _ => None,
        };
        let mut fg = default_bold.unwrap_or_else(|| self.color(fg_color));
        if f.contains(Flags::DIM) {
            fg = dim(fg);
        }
        let mut bg = self.color(cell.bg);
        if f.contains(Flags::INVERSE) {
            std::mem::swap(&mut fg, &mut bg);
        }
        if f.contains(Flags::HIDDEN) {
            fg = bg;
        }

        let mut flags = CellFlags::empty();
        flags.set(CellFlags::BOLD, bold);
        flags.set(CellFlags::ITALIC, f.contains(Flags::ITALIC));
        flags.set(
            CellFlags::UNDERLINE,
            f.intersects(Flags::UNDERLINE | Flags::DOTTED_UNDERLINE | Flags::DASHED_UNDERLINE),
        );
        flags.set(
            CellFlags::DOUBLE_UNDERLINE,
            f.contains(Flags::DOUBLE_UNDERLINE),
        );
        flags.set(CellFlags::UNDERCURL, f.contains(Flags::UNDERCURL));
        flags.set(CellFlags::STRIKEOUT, f.contains(Flags::STRIKEOUT));
        flags.set(CellFlags::INVERSE, f.contains(Flags::INVERSE));
        flags.set(CellFlags::DIM, f.contains(Flags::DIM));
        flags.set(CellFlags::HIDDEN, f.contains(Flags::HIDDEN));
        flags.set(CellFlags::WIDE, f.contains(Flags::WIDE_CHAR));
        flags.set(
            CellFlags::WIDE_SPACER,
            f.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER),
        );
        flags.set(
            CellFlags::DEFAULT_BACKGROUND,
            cell.bg == Color::Named(NamedColor::Background) && !f.contains(Flags::INVERSE),
        );

        GridCell {
            ch: if f.contains(Flags::HIDDEN) {
                ' '
            } else {
                cell.c
            },
            zerowidth: cell.zerowidth().map(<[char]>::to_vec).unwrap_or_default(),
            fg,
            bg,
            flags,
        }
    }
}
