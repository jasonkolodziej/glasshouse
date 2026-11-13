use std::path::PathBuf;

/// A `file://` URL reported by OSC 7, with the path percent-decoded.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FileUrl {
    /// Host component (`""` when absent, as in `file:///tmp`).
    pub host: String,
    /// Percent-decoded absolute path.
    pub path: String,
}

impl FileUrl {
    /// Parses a `file:` URL; `None` for other schemes or malformed percent escapes.
    pub fn parse(raw: &str) -> Option<Self> {
        let scheme_len = "file:".len();
        if raw.len() < scheme_len || !raw.is_char_boundary(scheme_len) {
            return None;
        }
        let (scheme, rest) = raw.split_at(scheme_len);
        if !scheme.eq_ignore_ascii_case("file:") {
            return None;
        }
        // Query and fragment are not part of the path.
        let rest = rest.split(['?', '#']).next().unwrap_or_default();
        let (host, path) = match rest.strip_prefix("//") {
            Some(authority_and_path) => match authority_and_path.find('/') {
                Some(i) => authority_and_path.split_at(i),
                None => (authority_and_path, ""),
            },
            None => ("", rest),
        };
        Some(Self {
            host: percent_decode(host)?,
            path: percent_decode(path)?,
        })
    }

    /// The path as a filesystem path.
    pub fn to_path_buf(&self) -> PathBuf {
        PathBuf::from(&self.path)
    }
}

fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = bytes.get(i + 1..i + 3)?;
            let hex = std::str::from_utf8(hex).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Semantic events emitted by the shell through OSC escape sequences.
///
/// - OSC 133 (FinalTerm semantic prompts): A = prompt start, B = input start,
///   C = output start (command executing), `D[;exit]` = command finished.
/// - OSC 7: current working directory as a file:// URL.
/// - OSC 9 / OSC 777;notify: desktop notifications (OSC 9 is ambiguous with ConEmu progress `9;4;…`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellEvent {
    /// OSC 133;A.
    PromptStart,
    /// OSC 133;B.
    InputStart,
    /// OSC 133;C.
    OutputStart,
    /// OSC 133;D with the optional exit status.
    CommandFinished {
        /// Exit status, if the shell reported one.
        exit_code: Option<i32>,
    },
    /// OSC 7.
    WorkingDirectory(FileUrl),
    /// OSC 9 or OSC 777;notify.
    Notification {
        /// Title (OSC 777 only).
        title: Option<String>,
        /// Body text.
        body: String,
    },
    /// ConEmu progress, OSC 9;4;state;percent.
    Progress {
        /// ConEmu progress state (0 = remove, 1 = set, 2 = error, 3 = indeterminate, 4 = paused).
        state: i64,
        /// Percent complete, if given.
        percent: Option<i64>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum State {
    #[default]
    Ground,
    Escape,
    Osc,
    OscEscape,
}

/// Guard against unterminated sequences eating memory.
const MAX_LENGTH: usize = 4096;

/// Incremental scanner that pulls OSC sequences out of the PTY byte stream without
/// consuming them — the bytes still go to the terminal engine unchanged.
///
/// Handles sequences split across reads, and both terminators (BEL `0x07` and ST `ESC \`).
#[derive(Debug, Clone, Default)]
pub struct OscScanner {
    state: State,
    buffer: Vec<u8>,
}

impl OscScanner {
    /// A scanner in the ground state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Scans a chunk of PTY output, returning the events completed within it.
    pub fn scan(&mut self, bytes: impl IntoIterator<Item = u8>) -> Vec<ShellEvent> {
        bytes
            .into_iter()
            .filter_map(|b| self.scan_byte(b))
            .collect()
    }

    /// Feeds one byte; returns an event when this byte terminated a recognised OSC sequence.
    ///
    /// Lets an engine interleave scanning with emulation so each event can be tagged with the
    /// cursor position at the exact point the sequence ended.
    pub fn scan_byte(&mut self, byte: u8) -> Option<ShellEvent> {
        match self.state {
            State::Ground => {
                if byte == 0x1B {
                    self.state = State::Escape;
                } else if byte == 0x9D {
                    // C1 OSC.
                    self.state = State::Osc;
                    self.buffer.clear();
                }
            }
            State::Escape => {
                if byte == b']' {
                    self.state = State::Osc;
                    self.buffer.clear();
                } else {
                    self.state = if byte == 0x1B {
                        State::Escape
                    } else {
                        State::Ground
                    };
                }
            }
            State::Osc => {
                if byte == 0x07 {
                    return self.finish();
                } else if byte == 0x1B {
                    self.state = State::OscEscape;
                } else if self.buffer.len() < MAX_LENGTH {
                    self.buffer.push(byte);
                } else {
                    self.state = State::Ground;
                    self.buffer = Vec::new();
                }
            }
            State::OscEscape => {
                if byte == b'\\' {
                    return self.finish();
                }
                self.state = State::Ground;
                self.buffer = Vec::new();
            }
        }
        None
    }

    fn finish(&mut self) -> Option<ShellEvent> {
        self.state = State::Ground;
        let event = std::str::from_utf8(&self.buffer).ok().and_then(Self::parse);
        self.buffer.clear();
        event
    }

    /// Parses an OSC payload (the text between `ESC ]` and the terminator).
    pub fn parse(payload: &str) -> Option<ShellEvent> {
        let parts: Vec<&str> = payload.split(';').collect();
        let code = *parts.first()?;
        match code {
            "133" => {
                let kind = parts.get(1)?.chars().next()?;
                match kind {
                    'A' => Some(ShellEvent::PromptStart),
                    'B' => Some(ShellEvent::InputStart),
                    'C' => Some(ShellEvent::OutputStart),
                    'D' => Some(ShellEvent::CommandFinished {
                        exit_code: parts.get(2).and_then(|s| s.parse().ok()),
                    }),
                    _ => None,
                }
            }
            "7" => FileUrl::parse(&parts[1..].join(";")).map(ShellEvent::WorkingDirectory),
            "9" => {
                // ConEmu progress: 9;4;state;percent
                if parts.len() >= 3
                    && parts[1] == "4"
                    && let Ok(state) = parts[2].parse()
                {
                    return Some(ShellEvent::Progress {
                        state,
                        percent: parts.get(3).and_then(|s| s.parse().ok()),
                    });
                }
                Some(ShellEvent::Notification {
                    title: None,
                    body: parts[1..].join(";"),
                })
            }
            "777" => {
                // 777;notify;title;body
                if parts.len() < 4 || parts[1] != "notify" {
                    return None;
                }
                Some(ShellEvent::Notification {
                    title: Some(parts[2].to_owned()),
                    body: parts[3..].join(";"),
                })
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(s: &str) -> Vec<u8> {
        s.as_bytes().to_vec()
    }

    #[test]
    fn parses_osc133_with_both_terminators() {
        let mut s = OscScanner::new();
        let events = s.scan(bytes(
            "\u{1B}]133;A\u{07}$ ls\u{1B}]133;C\u{1B}\\out\u{1B}]133;D;2\u{07}",
        ));
        assert_eq!(
            events,
            [
                ShellEvent::PromptStart,
                ShellEvent::OutputStart,
                ShellEvent::CommandFinished { exit_code: Some(2) }
            ]
        );
    }

    #[test]
    fn handles_sequences_split_across_reads() {
        let mut s = OscScanner::new();
        assert!(s.scan(bytes("text \u{1B}]13")).is_empty());
        assert!(s.scan(bytes("3;D;0")).is_empty());
        assert_eq!(
            s.scan(bytes("\u{07}more")),
            [ShellEvent::CommandFinished { exit_code: Some(0) }]
        );
    }

    #[test]
    fn distinguishes_osc9_notification_from_conemu_progress() {
        assert_eq!(
            OscScanner::parse("9;Build done"),
            Some(ShellEvent::Notification {
                title: None,
                body: "Build done".into()
            })
        );
        assert_eq!(
            OscScanner::parse("9;4;1;42"),
            Some(ShellEvent::Progress {
                state: 1,
                percent: Some(42)
            })
        );
    }

    #[test]
    fn parses_osc7_working_directory() {
        assert_eq!(
            OscScanner::parse("7;file://mac/Users/j/My%20Dir"),
            Some(ShellEvent::WorkingDirectory(FileUrl {
                host: "mac".into(),
                path: "/Users/j/My Dir".into()
            }))
        );
        assert_eq!(OscScanner::parse("7;https://example.com/"), None);
        assert_eq!(OscScanner::parse("7;file:///tmp/%ZZ"), None);
    }

    #[test]
    fn ignores_non_osc_escapes() {
        let mut s = OscScanner::new();
        assert!(s.scan(bytes("\u{1B}[31mred\u{1B}[0m")).is_empty());
    }

    #[test]
    fn parses_c1_osc_and_osc777() {
        let mut s = OscScanner::new();
        let mut input = vec![0x9D];
        input.extend(b"777;notify;Build;done;ok\x07");
        assert_eq!(
            s.scan(input),
            [ShellEvent::Notification {
                title: Some("Build".into()),
                body: "done;ok".into()
            }]
        );
        assert_eq!(
            OscScanner::parse("133;D"),
            Some(ShellEvent::CommandFinished { exit_code: None })
        );
        assert_eq!(OscScanner::parse("133"), None);
    }

    #[test]
    fn drops_oversized_sequences() {
        let mut s = OscScanner::new();
        let mut input = bytes("\u{1B}]133;A");
        input.extend(std::iter::repeat_n(b'x', MAX_LENGTH));
        input.push(0x07);
        assert!(s.scan(input).is_empty());
        assert_eq!(
            s.scan(bytes("\u{1B}]133;B\u{07}")),
            [ShellEvent::InputStart]
        );
    }
}
