//! Mouse selection: what a drag, a double click and a triple click select, and what Copy gets.

use terminal_core::{GridSize, PtyTerminal, PtyTerminalOptions, SelectionBounds, SelectionKind};

fn engine() -> PtyTerminal {
    let options = PtyTerminalOptions {
        size: GridSize::new(30, 4),
        ..PtyTerminalOptions::default()
    };
    let e = PtyTerminal::new(options).0;
    e.feed(b"echo hello world\r\nsecond line here");
    e
}

#[test]
fn a_drag_should_select_the_cells_it_crosses() {
    let e = engine();
    e.begin_selection(SelectionKind::Cells, 0, 5, false);
    e.extend_selection(0, 9, true);
    assert_eq!(e.selection_text().as_deref(), Some("hello"));
    assert_eq!(
        e.snapshot().selection,
        Some(SelectionBounds {
            start: (0, 5),
            end: (0, 9)
        })
    );
}

#[test]
fn a_drag_across_lines_should_join_them_with_a_newline() {
    let e = engine();
    e.begin_selection(SelectionKind::Cells, 0, 11, false);
    e.extend_selection(1, 5, true);
    assert_eq!(e.selection_text().as_deref(), Some("world\nsecond"));
}

#[test]
fn a_double_click_should_select_the_word() {
    let e = engine();
    e.begin_selection(SelectionKind::Words, 0, 7, false);
    assert_eq!(e.selection_text().as_deref(), Some("hello"));
}

#[test]
fn a_triple_click_should_select_the_line() {
    let e = engine();
    e.begin_selection(SelectionKind::Lines, 1, 3, false);
    assert_eq!(e.selection_text().as_deref(), Some("second line here\n"));
}

#[test]
fn shift_click_without_a_selection_should_start_one() {
    let e = engine();
    e.extend_selection(0, 0, false);
    e.extend_selection(0, 3, true);
    assert_eq!(e.selection_text().as_deref(), Some("echo"));
}

#[test]
fn a_click_without_a_drag_should_select_nothing() {
    let e = engine();
    e.begin_selection(SelectionKind::Cells, 0, 2, false);
    assert_eq!(e.selection_text(), None);
}

#[test]
fn select_all_should_take_every_line() {
    let e = engine();
    e.select_all();
    let text = e.selection_text().expect("a selection");
    assert!(
        text.starts_with("echo hello world\nsecond line here"),
        "{text:?}"
    );
}

#[test]
fn clearing_should_drop_the_selection() {
    let e = engine();
    e.begin_selection(SelectionKind::Words, 0, 7, false);
    e.clear_selection();
    assert_eq!(e.selection_text(), None);
    assert_eq!(e.snapshot().selection, None);
}

#[test]
fn bounds_should_give_each_row_its_selected_columns() {
    let b = SelectionBounds {
        start: (2, 4),
        end: (4, 1),
    };
    assert_eq!(b.columns_in(1, 10), None);
    assert_eq!(b.columns_in(2, 10), Some(4..10));
    assert_eq!(b.columns_in(3, 10), Some(0..10));
    assert_eq!(b.columns_in(4, 10), Some(0..2));
    assert_eq!(b.columns_in(5, 10), None);
}
