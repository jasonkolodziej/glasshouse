//! Pure text operations on ssh_config files. Nothing here touches the disk.

use std::collections::HashSet;

use crate::glasshouse_ssh;

/// First line of the managed block in `~/.ssh/config`.
pub const BEGIN_MARKER: &str = "# >>> glasshouse >>>";
/// Last line of the managed block in `~/.ssh/config`.
pub const END_MARKER: &str = "# <<< glasshouse <<<";

/// Goes first in `~/.ssh/config`. For most options ssh keeps the first value it finds, and an `Include`
/// after a `Host` line would only apply inside that block.
pub const INCLUDE_BLOCK: &str = "# >>> glasshouse >>>
# Managed by Glasshouse (Settings › SSH Keys). Keep this block above every Host and Match line.
Include ~/.ssh/glasshouse.conf
# <<< glasshouse <<<
";

/// One keyword line of an ssh_config file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshConfigDirective {
    /// 1-based line number.
    pub line: usize,
    /// Lowercased, as ssh treats keywords case-insensitively.
    pub keyword: String,
    /// The arguments, with quotes removed.
    pub arguments: Vec<String>,
}

/// Whether `~/.ssh/config` pulls in `~/.ssh/glasshouse.conf` where it takes effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SshIncludeStatus {
    /// No `Include` of the managed file.
    Missing,
    /// Before every `Host` and `Match` line: the managed hosts apply, and their values win over later blocks.
    Active,
    /// After a `Host` or `Match` line, so it only applies inside that block.
    Inactive {
        /// 1-based line of the `Include`.
        line: usize,
    },
}

/// Another `Host` block in `~/.ssh/config` that names a managed alias.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SshConfigConflict {
    /// The managed alias the other block names.
    pub alias: String,
    /// 1-based line of that block's `Host` line.
    pub line: usize,
}

impl SshConfigConflict {
    /// Stable identity: `alias:line`.
    pub fn id(&self) -> String {
        format!("{}:{}", self.alias, self.line)
    }
}

/// Every keyword line, with its line number.
pub fn directives(text: &str) -> Vec<SshConfigDirective> {
    lines(text)
        .enumerate()
        .filter_map(|(index, line)| {
            parse(line).map(|(keyword, arguments)| SshConfigDirective {
                line: index + 1,
                keyword,
                arguments,
            })
        })
        .collect()
}

/// Concrete `Host` names, in file order without repeats. Patterns (`*`, `?`) and negations are skipped.
pub fn host_aliases(text: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    directives(text)
        .into_iter()
        .filter(|directive| directive.keyword == "host")
        .flat_map(|directive| directive.arguments)
        .filter(|name| {
            !name.contains('*')
                && !name.contains('?')
                && !name.starts_with('!')
                && seen.insert(name.clone())
        })
        .collect()
}

/// Every argument of every `Include` line, unexpanded.
pub fn include_patterns(text: &str) -> Vec<String> {
    directives(text)
        .into_iter()
        .filter(|directive| directive.keyword == "include")
        .flat_map(|directive| directive.arguments)
        .collect()
}

/// Where, if anywhere, `text` includes the managed file.
pub fn include_status(text: &str) -> SshIncludeStatus {
    let all = directives(text);
    let Some(include) = all.iter().find(|d| {
        d.keyword == "include" && d.arguments.iter().any(|argument| is_our_include(argument))
    }) else {
        return SshIncludeStatus::Missing;
    };
    match all
        .iter()
        .find(|d| d.keyword == "host" || d.keyword == "match")
    {
        Some(first_block) if first_block.line < include.line => {
            SshIncludeStatus::Inactive { line: include.line }
        }
        _ => SshIncludeStatus::Active,
    }
}

/// Puts the managed block at the very top, moving it there if it is already further down.
/// Everything else is kept byte for byte. Running it twice gives the same text.
pub fn installing_include(text: &str) -> String {
    let rest = removing_include(text);
    if rest.is_empty() {
        INCLUDE_BLOCK.to_owned()
    } else {
        format!("{INCLUDE_BLOCK}\n{rest}")
    }
}

/// Removes the managed block, and the blank line [`installing_include`] put after it.
pub fn removing_include(text: &str) -> String {
    let mut raw: Vec<&str> = text.split('\n').collect();
    let Some(begin) = raw.iter().position(|line| strip_cr(line) == BEGIN_MARKER) else {
        return text.to_owned();
    };
    let Some(end) = raw[begin..]
        .iter()
        .position(|line| strip_cr(line) == END_MARKER)
        .map(|i| begin + i)
    else {
        return text.to_owned();
    };
    raw.drain(begin..=end);
    if begin < raw.len()
        && strip_cr(raw[begin]).is_empty()
        && (begin == 0 || strip_cr(raw[begin - 1]).is_empty())
    {
        raw.remove(begin);
    }
    raw.join("\n")
}

/// `Host` lines outside the managed block that name a managed alias. Glasshouse's entry is read first,
/// so its HostName, User and Port win, but the other block's remaining settings still apply.
pub fn conflicts<S: AsRef<str>>(text: &str, aliases: &[S]) -> Vec<SshConfigConflict> {
    let wanted: HashSet<&str> = aliases.iter().map(AsRef::as_ref).collect();
    let all: Vec<&str> = lines(text).collect();
    let managed = all
        .iter()
        .position(|line| *line == BEGIN_MARKER)
        .and_then(|begin| {
            all[begin..]
                .iter()
                .position(|line| *line == END_MARKER)
                .map(|end| (begin + 1)..=(begin + end + 1))
        });
    directives(text)
        .into_iter()
        .filter(|d| {
            d.keyword == "host"
                && !managed
                    .as_ref()
                    .is_some_and(|range| range.contains(&d.line))
        })
        .flat_map(|d| {
            d.arguments
                .into_iter()
                .filter(|alias| wanted.contains(alias.as_str()))
                .map(move |alias| SshConfigConflict {
                    alias,
                    line: d.line,
                })
        })
        .collect()
}

fn is_our_include(argument: &str) -> bool {
    argument == glasshouse_ssh::INCLUDE_PATH
        || argument == glasshouse_ssh::INCLUDE_FILE_NAME
        || argument.ends_with(&format!("/.ssh/{}", glasshouse_ssh::INCLUDE_FILE_NAME))
}

/// Lines split at "\n" only, without their "\r", for parsing. Joining the raw pieces with "\n" rebuilds a CRLF
/// file unchanged.
pub(crate) fn lines(text: &str) -> impl Iterator<Item = &str> {
    text.split('\n').map(strip_cr)
}

fn strip_cr(line: &str) -> &str {
    line.strip_suffix('\r').unwrap_or(line)
}

/// `Keyword arg …` or `Keyword=arg …`. Double quotes group an argument; `#` starts a comment.
pub(crate) fn parse(line: &str) -> Option<(String, Vec<String>)> {
    let is_space = |c: char| c == ' ' || c == '\t';
    let body = line.trim_start_matches(is_space);
    if body.is_empty() || body.starts_with('#') {
        return None;
    }
    let keyword_end = body.find(|c| is_space(c) || c == '=').unwrap_or(body.len());
    let mut rest = body[keyword_end..].trim_start_matches(is_space);
    if let Some(after) = rest.strip_prefix('=') {
        rest = after.trim_start_matches(is_space);
    }

    let mut arguments = Vec::new();
    let mut current = String::new();
    let (mut quoted, mut in_token) = (false, false);
    for c in rest.chars() {
        if c == '"' {
            quoted = !quoted;
            in_token = true;
        } else if is_space(c) && !quoted {
            if in_token {
                arguments.push(std::mem::take(&mut current));
                in_token = false;
            }
        } else if c == '#' && !quoted && !in_token {
            break;
        } else {
            current.push(c);
            in_token = true;
        }
    }
    if in_token {
        arguments.push(current);
    }
    Some((body[..keyword_end].to_lowercase(), arguments))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::FIXTURE;

    #[test]
    fn installs_the_include_above_everything_else() {
        let original = FIXTURE;
        assert_eq!(include_status(original), SshIncludeStatus::Missing);

        let updated = installing_include(original);
        assert!(updated.starts_with(&format!("{INCLUDE_BLOCK}\n")));
        assert!(updated.ends_with(original));
        assert_eq!(include_status(&updated), SshIncludeStatus::Active);
        assert_eq!(installing_include(&updated), updated);
    }

    #[test]
    fn removing_restores_the_original_exactly() {
        assert_eq!(removing_include(&installing_include(FIXTURE)), FIXTURE);
        assert_eq!(installing_include(""), INCLUDE_BLOCK);
        assert_eq!(removing_include(INCLUDE_BLOCK), "");
    }

    #[test]
    fn moves_a_block_that_ended_up_below_a_host_line() {
        let misplaced = format!("{FIXTURE}\n{INCLUDE_BLOCK}");
        let SshIncludeStatus::Inactive { line } = include_status(&misplaced) else {
            panic!("expected Inactive");
        };
        assert_eq!(
            lines(&misplaced).nth(line - 1),
            Some("Include ~/.ssh/glasshouse.conf")
        );

        let fixed = installing_include(&misplaced);
        assert_eq!(include_status(&fixed), SshIncludeStatus::Active);
        assert_eq!(fixed.split(BEGIN_MARKER).count(), 2);
    }

    #[test]
    fn keeps_windows_line_endings() {
        let original = FIXTURE.replace('\n', "\r\n");
        let updated = installing_include(&original);
        assert!(updated.ends_with(&original));
        assert_eq!(include_status(&updated), SshIncludeStatus::Active);
        assert_eq!(host_aliases(&original), ["myserver", "myserver.local"]);
        assert_eq!(removing_include(&updated), original);
    }

    #[test]
    fn accepts_a_hand_written_include_at_the_top() {
        assert_eq!(
            include_status("Include glasshouse.conf\nHost a\n"),
            SshIncludeStatus::Active
        );
        assert_eq!(
            include_status("Host a\n  Include ~/.ssh/glasshouse.conf\n"),
            SshIncludeStatus::Inactive { line: 2 }
        );
    }

    #[test]
    fn finds_other_blocks_for_a_managed_alias() {
        let text = installing_include(FIXTURE);
        let expected: Vec<usize> = lines(&text)
            .enumerate()
            .filter(|(_, line)| *line == "Host myserver")
            .map(|(i, _)| i + 1)
            .collect();
        assert_eq!(expected.len(), 2);
        let found: Vec<usize> = conflicts(&text, &["myserver"])
            .iter()
            .map(|c| c.line)
            .collect();
        assert_eq!(found, expected);
        assert!(conflicts(&text, &["elsewhere"]).is_empty());
    }

    #[test]
    fn reads_aliases_and_includes() {
        assert_eq!(host_aliases(FIXTURE), ["myserver", "myserver.local"]);
        assert_eq!(include_patterns(FIXTURE), ["~/.ssh/arc-slurm.conf"]);
    }

    #[test]
    fn parses_the_ways_ssh_allows_lines_to_be_written() {
        for (line, keyword, arguments) in [
            ("  HOST=a b", "host", vec!["a", "b"]),
            (
                "\tIdentityFile \"~/My Keys/id\" # work",
                "identityfile",
                vec!["~/My Keys/id"],
            ),
            ("Port = 22", "port", vec!["22"]),
        ] {
            let (parsed_keyword, parsed_arguments) = parse(line).unwrap();
            assert_eq!(parsed_keyword, keyword, "{line:?}");
            assert_eq!(parsed_arguments, arguments, "{line:?}");
        }
    }

    #[test]
    fn skips_comments_and_blank_lines_and_strips_carriage_returns() {
        assert_eq!(parse("   # comment"), None);
        assert_eq!(parse(""), None);
        assert_eq!(
            directives("Port 22\r\n")
                .first()
                .map(|d| d.arguments.clone()),
            Some(vec!["22".to_owned()])
        );
    }
}
