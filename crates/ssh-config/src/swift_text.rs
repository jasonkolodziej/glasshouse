//! Small helpers that reproduce Foundation's string behaviour where it matters for a faithful port.

/// `CharacterSet.whitespaces`: tab plus Unicode `Zs`, without line breaks.
pub(crate) fn is_swift_whitespace(c: char) -> bool {
    c.is_whitespace() && !is_newline(c)
}

/// `Character.isNewline`.
pub(crate) fn is_newline(c: char) -> bool {
    matches!(
        c,
        '\n' | '\r' | '\u{0B}' | '\u{0C}' | '\u{85}' | '\u{2028}' | '\u{2029}'
    )
}

/// `trimmingCharacters(in: .whitespaces)`.
pub(crate) fn trim_whitespace(s: &str) -> &str {
    s.trim_matches(is_swift_whitespace)
}

/// `split(maxSplits:whereSeparator:)` with empty pieces omitted: once `max_splits` pieces are found, the rest of
/// the string (leading separators included) is the last piece.
pub(crate) fn split_max(
    s: &str,
    max_splits: usize,
    is_separator: impl Fn(char) -> bool,
) -> Vec<&str> {
    let mut result = Vec::new();
    let mut start = 0;
    for (index, c) in s.char_indices() {
        if !is_separator(c) {
            continue;
        }
        let appended = index > start;
        if appended {
            result.push(&s[start..index]);
        }
        start = index + c.len_utf8();
        if appended && result.len() == max_splits {
            break;
        }
    }
    if start < s.len() {
        result.push(&s[start..]);
    }
    result
}

/// The first non-empty line, splitting at every line break as `split(whereSeparator: \.isNewline)` does.
pub(crate) fn first_line(s: &str) -> Option<&str> {
    s.split(is_newline).find(|line| !line.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_max_keeps_the_remainder_whole() {
        let space = |c| c == ' ';
        assert_eq!(split_max("a  b  c d", 2, space), ["a", "b", " c d"]);
        assert_eq!(split_max("  a b", 2, space), ["a", "b"]);
        assert_eq!(split_max("a", 2, space), ["a"]);
        assert!(split_max("", 2, space).is_empty());
    }
}
