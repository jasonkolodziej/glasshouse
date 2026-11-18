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

fn long_output(e: &PtyTerminal) {
    for i in 0..50 {
        e.feed(format!("line {i}\r\n").as_bytes());
    }
}

#[test]
fn wheel_on_the_primary_screen_should_scroll_back_through_history() {
    let e = engine(None, false);
    long_output(&e);
    let bottom = e.top_visible_row();
    e.wheel(5, 0, 0, terminal_core::Modifiers::default());
    assert_eq!(e.top_visible_row(), bottom - 5);
    assert_eq!(e.snapshot().display_offset, 5);
    e.wheel(-2, 0, 0, terminal_core::Modifiers::default());
    assert_eq!(e.top_visible_row(), bottom - 3);
}

#[test]
fn wheel_should_stop_at_the_ends_of_history() {
    let e = engine(None, false);
    long_output(&e);
    e.wheel(-10, 0, 0, terminal_core::Modifiers::default());
    assert_eq!(e.snapshot().display_offset, 0);
    e.wheel(10_000, 0, 0, terminal_core::Modifiers::default());
    assert_eq!(e.top_visible_row(), 0);
}

#[test]
fn wheel_on_the_alternate_screen_should_leave_the_viewport_to_the_program() {
    let e = engine(None, false);
    long_output(&e);
    e.feed(b"\x1b[?1049h");
    e.wheel(5, 0, 0, terminal_core::Modifiers::default());
    assert_eq!(e.snapshot().display_offset, 0);
}

#[test]
fn output_arriving_while_scrolled_back_should_keep_the_view_still() {
    let e = engine(None, false);
    long_output(&e);
    e.wheel(5, 0, 0, terminal_core::Modifiers::default());
    let top = e.top_visible_row();
    e.feed(b"more\r\n");
    assert_eq!(e.top_visible_row(), top);
}
