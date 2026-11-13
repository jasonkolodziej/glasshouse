//! Output fed to the engine without a shell (`PtyTerminal::feed`): what demo tabs and snapshot
//! scenes rely on, and the colour rules that don't need a PTY to check.

use design_system::Rgba;
use terminal_core::{
    GridSize, PtyTerminal, PtyTerminalOptions, TerminalAppearance, TerminalEngine,
};

const BOLD: Rgba = Rgba::rgb(1.0, 0.5, 0.0);

fn engine(bold: Option<Rgba>, bold_is_bright: bool) -> PtyTerminal {
    let appearance = TerminalAppearance {
        bold,
        bold_is_bright,
        ..TerminalAppearance::default()
    };
    let options = PtyTerminalOptions {
        size: GridSize::new(20, 3),
        appearance,
        ..PtyTerminalOptions::default()
    };
    PtyTerminal::new(options).0
}

fn first_cell_fg(engine: &PtyTerminal) -> Rgba {
    engine.snapshot().lines[0].cells[0].fg
}

#[test]
fn fed_text_should_reach_the_grid() {
    let e = engine(None, true);
    e.feed(b"hello");
    assert_eq!(e.text(0, 1), "hello");
}

#[test]
fn bold_default_text_should_take_the_theme_bold_colour() {
    let e = engine(Some(BOLD), true);
    e.feed(b"\x1b[1mB");
    assert_eq!(first_cell_fg(&e), BOLD);
}

#[test]
fn bold_text_in_an_explicit_colour_should_keep_it() {
    let e = engine(Some(BOLD), false);
    e.feed(b"\x1b[1;31mR");
    assert_eq!(first_cell_fg(&e), TerminalAppearance::default().ansi[1]);
}

#[test]
fn bold_default_text_without_a_bold_colour_should_use_the_foreground() {
    let e = engine(None, false);
    e.feed(b"\x1b[1mB");
    assert_eq!(first_cell_fg(&e), TerminalAppearance::default().foreground);
}
