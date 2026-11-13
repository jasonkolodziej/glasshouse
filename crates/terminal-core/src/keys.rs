/// A logical key press, after the platform's keyboard layout has been applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Key {
    /// Text produced by the key (already reflecting Shift / layout), e.g. `"a"`, `"A"`, `"é"`.
    Text(String),
    /// Return.
    Enter,
    /// Delete-backward.
    Backspace,
    /// Tab (Shift+Tab sends back-tab).
    Tab,
    /// Escape.
    Escape,
    /// Up arrow.
    Up,
    /// Down arrow.
    Down,
    /// Left arrow.
    Left,
    /// Right arrow.
    Right,
    /// Home.
    Home,
    /// End.
    End,
    /// Page Up.
    PageUp,
    /// Page Down.
    PageDown,
    /// Insert / Help.
    Insert,
    /// Forward delete.
    Delete,
    /// Function key F1…F12 (other numbers produce nothing).
    F(u8),
}

/// Modifier state for [`encode_key`]. Alt is Option-as-Meta; the UI decides when Option composes text instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    /// Shift.
    pub shift: bool,
    /// Control.
    pub control: bool,
    /// Alt / Option (as Meta).
    pub alt: bool,
}

impl Modifiers {
    /// xterm modifier parameter: 1 + shift + 2·alt + 4·ctrl.
    fn xterm_param(self) -> u8 {
        1 + u8::from(self.shift) + 2 * u8::from(self.alt) + 4 * u8::from(self.control)
    }
}

const ESC: u8 = 0x1B;

/// Converts a key press to the bytes an xterm-compatible terminal sends.
///
/// `app_cursor` is DECCKM (application cursor keys, [`TerminalModes::app_cursor`](crate::TerminalModes)):
/// unmodified arrows and Home/End send `ESC O x` instead of `ESC [ x`.
pub fn encode_key(key: &Key, mods: Modifiers, app_cursor: bool) -> Vec<u8> {
    let alt_prefixed = |bytes: &[u8]| {
        let mut out = Vec::with_capacity(bytes.len() + 1);
        if mods.alt {
            out.push(ESC);
        }
        out.extend_from_slice(bytes);
        out
    };
    match key {
        Key::Text(text) => {
            let control_byte = if mods.control {
                single_char(text).and_then(control_code)
            } else {
                None
            };
            match control_byte {
                Some(byte) => alt_prefixed(&[byte]),
                None => alt_prefixed(text.as_bytes()),
            }
        }
        Key::Enter => alt_prefixed(b"\r"),
        Key::Backspace => alt_prefixed(if mods.control { b"\x08" } else { b"\x7f" }),
        Key::Tab if mods.shift => b"\x1b[Z".to_vec(),
        Key::Tab => alt_prefixed(b"\t"),
        Key::Escape => alt_prefixed(&[ESC]),
        Key::Up => cursor_key(b'A', mods, app_cursor),
        Key::Down => cursor_key(b'B', mods, app_cursor),
        Key::Right => cursor_key(b'C', mods, app_cursor),
        Key::Left => cursor_key(b'D', mods, app_cursor),
        Key::Home => cursor_key(b'H', mods, app_cursor),
        Key::End => cursor_key(b'F', mods, app_cursor),
        Key::Insert => tilde_key(2, mods),
        Key::Delete => tilde_key(3, mods),
        Key::PageUp => tilde_key(5, mods),
        Key::PageDown => tilde_key(6, mods),
        Key::F(n @ 1..=4) => {
            let final_byte = b'P' + (n - 1);
            match mods.xterm_param() {
                1 => vec![ESC, b'O', final_byte],
                m => format!("\x1b[1;{m}{}", char::from(final_byte)).into_bytes(),
            }
        }
        Key::F(n @ 5..=12) => {
            const CODES: [u8; 8] = [15, 17, 18, 19, 20, 21, 23, 24];
            tilde_key(CODES[usize::from(n - 5)], mods)
        }
        Key::F(_) => Vec::new(),
    }
}

/// Encodes pasted text. With bracketed paste (DECSET 2004) the text is wrapped in `ESC[200~ … ESC[201~`
/// and stripped of ESC so it can't end the bracket early; otherwise newlines become carriage returns.
pub fn encode_paste(text: &str, bracketed: bool) -> Vec<u8> {
    if bracketed {
        let mut out = b"\x1b[200~".to_vec();
        out.extend(text.bytes().filter(|&b| b != ESC));
        out.extend_from_slice(b"\x1b[201~");
        out
    } else {
        text.replace("\r\n", "\r").replace('\n', "\r").into_bytes()
    }
}

fn cursor_key(final_byte: u8, mods: Modifiers, app_cursor: bool) -> Vec<u8> {
    match mods.xterm_param() {
        1 if app_cursor => vec![ESC, b'O', final_byte],
        1 => vec![ESC, b'[', final_byte],
        m => format!("\x1b[1;{m}{}", char::from(final_byte)).into_bytes(),
    }
}

fn tilde_key(code: u8, mods: Modifiers) -> Vec<u8> {
    match mods.xterm_param() {
        1 => format!("\x1b[{code}~").into_bytes(),
        m => format!("\x1b[{code};{m}~").into_bytes(),
    }
}

fn single_char(text: &str) -> Option<char> {
    let mut chars = text.chars();
    let c = chars.next()?;
    chars.next().is_none().then_some(c)
}

/// The C0 byte for Control+`c`, following xterm/Terminal.app conventions.
fn control_code(c: char) -> Option<u8> {
    Some(match c {
        'a'..='z' => c as u8 - b'a' + 1,
        'A'..='Z' => c as u8 - b'A' + 1,
        '@' | ' ' | '2' | '`' => 0x00,
        '[' | '3' | '{' => 0x1B,
        '\\' | '4' | '|' => 0x1C,
        ']' | '5' | '}' => 0x1D,
        '^' | '6' | '~' => 0x1E,
        '_' | '7' | '-' | '/' => 0x1F,
        '8' | '?' => 0x7F,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Modifiers = Modifiers {
        shift: false,
        control: false,
        alt: false,
    };
    const CTRL: Modifiers = Modifiers {
        shift: false,
        control: true,
        alt: false,
    };
    const ALT: Modifiers = Modifiers {
        shift: false,
        control: false,
        alt: true,
    };
    const SHIFT: Modifiers = Modifiers {
        shift: true,
        control: false,
        alt: false,
    };

    fn text(s: &str) -> Key {
        Key::Text(s.into())
    }

    #[test]
    fn plain_text_and_editing_keys() {
        assert_eq!(encode_key(&text("é"), NONE, false), "é".as_bytes());
        assert_eq!(encode_key(&Key::Enter, NONE, false), b"\r");
        assert_eq!(encode_key(&Key::Backspace, NONE, false), b"\x7f");
        assert_eq!(encode_key(&Key::Backspace, CTRL, false), b"\x08");
        assert_eq!(encode_key(&Key::Tab, NONE, false), b"\t");
        assert_eq!(encode_key(&Key::Tab, SHIFT, false), b"\x1b[Z");
        assert_eq!(encode_key(&Key::Escape, NONE, false), b"\x1b");
        assert_eq!(encode_key(&Key::Delete, NONE, false), b"\x1b[3~");
        assert_eq!(encode_key(&Key::PageUp, NONE, false), b"\x1b[5~");
        assert_eq!(encode_key(&Key::PageDown, SHIFT, false), b"\x1b[6;2~");
    }

    #[test]
    fn control_and_alt_combinations() {
        assert_eq!(encode_key(&text("c"), CTRL, false), b"\x03");
        assert_eq!(encode_key(&text("C"), CTRL, false), b"\x03");
        assert_eq!(encode_key(&text("["), CTRL, false), b"\x1b");
        assert_eq!(encode_key(&text(" "), CTRL, false), b"\x00");
        assert_eq!(encode_key(&text("x"), ALT, false), b"\x1bx");
        assert_eq!(
            encode_key(&text("d"), Modifiers { alt: true, ..CTRL }, false),
            b"\x1b\x04"
        );
        assert_eq!(encode_key(&Key::Backspace, ALT, false), b"\x1b\x7f");
        assert_eq!(encode_key(&text("ab"), CTRL, false), b"ab");
    }

    #[test]
    fn cursor_keys_respect_decckm_and_modifiers() {
        assert_eq!(encode_key(&Key::Up, NONE, false), b"\x1b[A");
        assert_eq!(encode_key(&Key::Up, NONE, true), b"\x1bOA");
        assert_eq!(encode_key(&Key::Left, NONE, true), b"\x1bOD");
        assert_eq!(encode_key(&Key::Home, NONE, false), b"\x1b[H");
        assert_eq!(encode_key(&Key::End, NONE, true), b"\x1bOF");
        assert_eq!(encode_key(&Key::Right, CTRL, true), b"\x1b[1;5C");
        assert_eq!(encode_key(&Key::Left, ALT, false), b"\x1b[1;3D");
    }

    #[test]
    fn function_keys() {
        assert_eq!(encode_key(&Key::F(1), NONE, false), b"\x1bOP");
        assert_eq!(encode_key(&Key::F(4), NONE, false), b"\x1bOS");
        assert_eq!(encode_key(&Key::F(2), SHIFT, false), b"\x1b[1;2Q");
        assert_eq!(encode_key(&Key::F(5), NONE, false), b"\x1b[15~");
        assert_eq!(encode_key(&Key::F(11), NONE, false), b"\x1b[23~");
        assert_eq!(encode_key(&Key::F(12), CTRL, false), b"\x1b[24;5~");
        assert!(encode_key(&Key::F(13), NONE, false).is_empty());
    }

    #[test]
    fn paste_encoding() {
        assert_eq!(encode_paste("a\nb\r\nc", false), b"a\rb\rc");
        assert_eq!(
            encode_paste("x\x1b[201~y", true),
            b"\x1b[200~x[201~y\x1b[201~"
        );
    }
}
