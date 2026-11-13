//! Keyboard routing: which keys are app shortcuts and which go to the shell.
//!
//! On macOS the app owns ⌘ and the shell gets Control, as in Terminal.app. Elsewhere Ctrl belongs to the
//! shell (Ctrl+C must interrupt), so app shortcuts move to Ctrl+Shift, as in GNOME Terminal and Konsole.

use rune::{Key, KeyEvent};
use terminal_core::{Key as TermKey, Modifiers};

/// An app-level command bound to a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shortcut {
    NewTab,
    CloseTab,
    CloseWindow,
    CommandPalette,
    OpenHost,
    PreviousPrompt,
    NextPrompt,
    CopyLastOutput,
    Clear,
    Find,
    ToggleSidebar,
    Settings,
    Copy,
    Paste,
    SelectTab(usize),
    NextTab,
    PreviousTab,
}

/// The shortcut `ev` triggers on this platform, if any.
pub fn shortcut(ev: &KeyEvent) -> Option<Shortcut> {
    shortcut_for(ev, cfg!(target_os = "macos"))
}

/// `shortcut` with the platform as a parameter, so both tables are tested everywhere.
pub fn shortcut_for(ev: &KeyEvent, mac: bool) -> Option<Shortcut> {
    let m = ev.mods;
    if mac {
        if !m.cmd {
            return None;
        }
        // ⌃⌘S is the only binding that also uses Control.
        if m.ctrl {
            return (ev.key == Key::Command('s')).then_some(Shortcut::ToggleSidebar);
        }
        return match (&ev.key, m.shift) {
            (Key::Command('t'), false) => Some(Shortcut::NewTab),
            (Key::Command('w'), false) => Some(Shortcut::CloseTab),
            (Key::Command('w'), true) => Some(Shortcut::CloseWindow),
            (Key::Command('p'), true) => Some(Shortcut::CommandPalette),
            (Key::Command('o'), true) => Some(Shortcut::OpenHost),
            (Key::Command('a'), true) => Some(Shortcut::CopyLastOutput),
            (Key::Command('k'), false) => Some(Shortcut::Clear),
            (Key::Command('f'), false) => Some(Shortcut::Find),
            (Key::Command(','), false) => Some(Shortcut::Settings),
            (Key::Command('c'), false) => Some(Shortcut::Copy),
            (Key::Command('v'), false) => Some(Shortcut::Paste),
            (Key::Command(']'), true) => Some(Shortcut::NextTab),
            (Key::Command('['), true) => Some(Shortcut::PreviousTab),
            (Key::Command(c @ '1'..='9'), false) => Some(Shortcut::SelectTab(digit(*c))),
            (Key::Up, false) => Some(Shortcut::PreviousPrompt),
            (Key::Down, false) => Some(Shortcut::NextPrompt),
            _ => None,
        };
    }
    // Off macOS `cmd` is Ctrl. Without Shift the chord belongs to the shell, except Ctrl+, and Ctrl+Tab.
    if !m.cmd {
        return None;
    }
    match (&ev.key, m.shift) {
        (Key::Command(','), false) => Some(Shortcut::Settings),
        (Key::Tab, false) => Some(Shortcut::NextTab),
        (Key::Tab, true) => Some(Shortcut::PreviousTab),
        (Key::Command('t'), true) => Some(Shortcut::NewTab),
        (Key::Command('w'), true) => Some(Shortcut::CloseTab),
        (Key::Command('q'), true) => Some(Shortcut::CloseWindow),
        (Key::Command('p'), true) => Some(Shortcut::CommandPalette),
        (Key::Command('o'), true) => Some(Shortcut::OpenHost),
        (Key::Command('a'), true) => Some(Shortcut::CopyLastOutput),
        (Key::Command('k'), true) => Some(Shortcut::Clear),
        (Key::Command('f'), true) => Some(Shortcut::Find),
        (Key::Command('s'), true) => Some(Shortcut::ToggleSidebar),
        (Key::Command('c'), true) => Some(Shortcut::Copy),
        (Key::Command('v'), true) => Some(Shortcut::Paste),
        (Key::Up, true) => Some(Shortcut::PreviousPrompt),
        (Key::Down, true) => Some(Shortcut::NextPrompt),
        (Key::Command(c @ '1'..='9'), false) if m.alt => Some(Shortcut::SelectTab(digit(*c))),
        _ => None,
    }
}

fn digit(c: char) -> usize {
    c.to_digit(10).map_or(0, |d| d as usize - 1)
}

/// The label shown next to a command (palette rows, help text), in this platform's notation.
pub fn label(shortcut: Shortcut) -> &'static str {
    label_for(shortcut, cfg!(target_os = "macos"))
}

pub fn label_for(shortcut: Shortcut, mac: bool) -> &'static str {
    use Shortcut as S;
    match (shortcut, mac) {
        (S::NewTab, true) => "⌘T",
        (S::NewTab, false) => "Ctrl+Shift+T",
        (S::CloseTab, true) => "⌘W",
        (S::CloseTab, false) => "Ctrl+Shift+W",
        (S::CloseWindow, true) => "⇧⌘W",
        (S::CloseWindow, false) => "Ctrl+Shift+Q",
        (S::CommandPalette, true) => "⇧⌘P",
        (S::CommandPalette, false) => "Ctrl+Shift+P",
        (S::OpenHost, true) => "⇧⌘O",
        (S::OpenHost, false) => "Ctrl+Shift+O",
        (S::PreviousPrompt, true) => "⌘↑",
        (S::PreviousPrompt, false) => "Ctrl+Shift+↑",
        (S::NextPrompt, true) => "⌘↓",
        (S::NextPrompt, false) => "Ctrl+Shift+↓",
        (S::CopyLastOutput, true) => "⇧⌘A",
        (S::CopyLastOutput, false) => "Ctrl+Shift+A",
        (S::Clear, true) => "⌘K",
        (S::Clear, false) => "Ctrl+Shift+K",
        (S::Find, true) => "⌘F",
        (S::Find, false) => "Ctrl+Shift+F",
        (S::ToggleSidebar, true) => "⌃⌘S",
        (S::ToggleSidebar, false) => "Ctrl+Shift+S",
        (S::Settings, true) => "⌘,",
        (S::Settings, false) => "Ctrl+,",
        (S::Copy, true) => "⌘C",
        (S::Copy, false) => "Ctrl+Shift+C",
        (S::Paste, true) => "⌘V",
        (S::Paste, false) => "Ctrl+Shift+V",
        (S::NextTab, true) => "⇧⌘]",
        (S::NextTab, false) => "Ctrl+Tab",
        (S::PreviousTab, true) => "⇧⌘[",
        (S::PreviousTab, false) => "Ctrl+Shift+Tab",
        (S::SelectTab(_), true) => "⌘1…9",
        (S::SelectTab(_), false) => "Alt+Ctrl+1…9",
    }
}

/// The key the shell receives for `ev`, or `None` for keys the terminal doesn't use.
/// Call only after `shortcut` returned `None`.
pub fn terminal_key(ev: &KeyEvent) -> Option<(TermKey, Modifiers)> {
    let m = ev.mods;
    let mods = Modifiers { shift: m.shift, control: m.ctrl, alt: m.alt };
    let key = match &ev.key {
        Key::Text(t) => TermKey::Text(t.clone()),
        // Off macOS, Ctrl+letter arrives as a command chord; to the shell it is a control character.
        Key::Command(c) if m.ctrl => TermKey::Text(c.to_string()),
        Key::Enter => TermKey::Enter,
        Key::Backspace => TermKey::Backspace,
        Key::Delete => TermKey::Delete,
        Key::Left => TermKey::Left,
        Key::Right => TermKey::Right,
        Key::Up => TermKey::Up,
        Key::Down => TermKey::Down,
        Key::Home => TermKey::Home,
        Key::End => TermKey::End,
        Key::PageUp => TermKey::PageUp,
        Key::PageDown => TermKey::PageDown,
        Key::Insert => TermKey::Insert,
        Key::Escape => TermKey::Escape,
        Key::Tab => TermKey::Tab,
        Key::Function(n) => TermKey::F(*n),
        _ => return None,
    };
    Some((key, mods))
}

#[cfg(test)]
mod tests {
    use rune::Mods;

    use super::*;

    fn ev(key: Key, shift: bool, cmd: bool, ctrl: bool) -> KeyEvent {
        KeyEvent { key, mods: Mods { shift, cmd, alt: false, ctrl, logo: cmd && !ctrl } }
    }

    #[test]
    fn mac_cmd_t_should_open_a_new_tab() {
        assert_eq!(shortcut_for(&ev(Key::Command('t'), false, true, false), true), Some(Shortcut::NewTab));
    }

    #[test]
    fn mac_shift_cmd_w_should_close_the_window_not_the_tab() {
        assert_eq!(
            shortcut_for(&ev(Key::Command('w'), true, true, false), true),
            Some(Shortcut::CloseWindow)
        );
    }

    #[test]
    fn mac_ctrl_cmd_s_should_toggle_the_sidebar() {
        assert_eq!(
            shortcut_for(&ev(Key::Command('s'), false, true, true), true),
            Some(Shortcut::ToggleSidebar)
        );
    }

    #[test]
    fn linux_ctrl_c_should_go_to_the_shell() {
        let e = ev(Key::Command('c'), false, true, true);
        assert_eq!(shortcut_for(&e, false), None);
        assert_eq!(terminal_key(&e).map(|(k, _)| k), Some(TermKey::Text("c".into())));
    }

    #[test]
    fn linux_ctrl_shift_t_should_open_a_new_tab() {
        assert_eq!(shortcut_for(&ev(Key::Command('t'), true, true, true), false), Some(Shortcut::NewTab));
    }

    #[test]
    fn linux_ctrl_shift_up_should_jump_to_previous_prompt() {
        assert_eq!(shortcut_for(&ev(Key::Up, true, true, true), false), Some(Shortcut::PreviousPrompt));
    }

    #[test]
    fn terminal_key_should_carry_control_for_mac_ctrl_letters() {
        let (_, mods) = terminal_key(&ev(Key::Text("c".into()), false, false, true)).expect("a terminal key");
        assert!(mods.control);
    }

    #[test]
    fn cmd_digit_should_select_the_matching_tab() {
        assert_eq!(
            shortcut_for(&ev(Key::Command('3'), false, true, false), true),
            Some(Shortcut::SelectTab(2))
        );
    }
}
