/// A shell command split into words, with heredoc bodies set aside.
///
/// Heredoc bodies are prose, and prose has apostrophes that look like unbalanced quotes
/// to a word splitter. They are removed before splitting and still scanned as part of
/// the command text, so setting them aside loses nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ShellWords {
    pub(crate) words: Vec<String>,
    /// Whether the command feeds anything a heredoc, which is a readable stdin body.
    pub(crate) has_heredoc: bool,
}

impl ShellWords {
    /// Splits `command`, or `None` when its quoting does not parse.
    pub(crate) fn split(command: &str) -> Option<Self> {
        let (without_bodies, has_heredoc) = set_aside_heredoc_bodies(command);
        let words = shlex::split(&separate_operators(&without_bodies))?;
        Some(Self { words, has_heredoc })
    }
}

/// Puts spaces around unquoted shell operators, and turns unquoted newlines into `;`.
///
/// shlex splits on whitespace only, so `a;` would otherwise be one word and the command
/// after it would read as more arguments to the one before. An `&` belonging to a
/// redirection (`2>&1`, `&>file`) is left in place.
fn separate_operators(command: &str) -> String {
    let chars: Vec<char> = command.chars().collect();
    let mut out = String::with_capacity(command.len());
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;

    for (i, &c) in chars.iter().enumerate() {
        let quoted = in_single || in_double;
        match c {
            _ if escaped => escaped = false,
            '\\' if !in_single => escaped = true,
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '\n' if !quoted => {
                out.push_str(" ; ");
                continue;
            }
            ';' | '|' | '(' | ')' if !quoted => {
                out.push(' ');
                out.push(c);
                out.push(' ');
                continue;
            }
            '&' if !quoted => {
                let previous = i.checked_sub(1).and_then(|p| chars.get(p));
                let next = chars.get(i + 1);
                let in_redirection = matches!(previous, Some('>' | '<')) || next == Some(&'>');
                if !in_redirection {
                    out.push_str(" & ");
                    continue;
                }
            }
            _ => {}
        }
        out.push(c);
    }
    out
}

/// A heredoc opened on a line, waiting for its delimiter.
struct OpenHeredoc {
    delimiter: String,
    /// `<<-` allows the closing delimiter to be indented with tabs.
    indented: bool,
}

/// The command with every heredoc body removed, and whether there was one.
///
/// The `<<` lines stay, so the words around them still split. The bodies go, because they
/// are prose rather than shell and their apostrophes would not parse.
fn set_aside_heredoc_bodies(command: &str) -> (String, bool) {
    let mut kept: Vec<&str> = Vec::new();
    let mut pending: Vec<OpenHeredoc> = Vec::new();
    let mut has_heredoc = false;

    for line in command.lines() {
        if let Some(open) = pending.first() {
            let candidate = if open.indented {
                line.trim_start_matches('\t')
            } else {
                line
            };
            if candidate == open.delimiter {
                let _closed = pending.remove(0);
            }
            continue;
        }
        kept.push(line);
        let opened = heredocs_opened_by(line);
        has_heredoc |= !opened.is_empty();
        pending.extend(opened);
    }

    (kept.join("\n"), has_heredoc)
}

/// The heredocs a line opens, in order. A `<<` inside quotes, or a `<<<` here-string, opens none.
fn heredocs_opened_by(line: &str) -> Vec<OpenHeredoc> {
    let chars: Vec<char> = line.chars().collect();
    let mut opened = Vec::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut i = 0;

    while let Some(&c) = chars.get(i) {
        match c {
            '\\' if !in_single => i += 1,
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '<' if !in_single && !in_double && chars.get(i + 1) == Some(&'<') => {
                if chars.get(i + 2) == Some(&'<') {
                    i += 3;
                    continue;
                }
                let (heredoc, next) = read_delimiter(&chars, i + 2);
                opened.extend(heredoc);
                i = next;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    opened
}

/// Reads the delimiter after a `<<` that starts at `start`, returning where reading stopped.
fn read_delimiter(chars: &[char], start: usize) -> (Option<OpenHeredoc>, usize) {
    let mut i = start;
    let indented = chars.get(i) == Some(&'-');
    if indented {
        i += 1;
    }
    while chars.get(i).is_some_and(|c| *c == ' ' || *c == '\t') {
        i += 1;
    }
    let quote = chars.get(i).copied().filter(|c| *c == '\'' || *c == '"');
    if quote.is_some() {
        i += 1;
    }
    let mut delimiter = String::new();
    while let Some(&c) = chars.get(i) {
        let ends = quote.map_or_else(
            || c.is_whitespace() || matches!(c, ';' | '&' | '|' | '<' | '>' | ')'),
            |q| c == q,
        );
        if ends {
            break;
        }
        delimiter.push(c);
        i += 1;
    }
    if quote.is_some() {
        i += 1;
    }
    let heredoc = (!delimiter.is_empty()).then_some(OpenHeredoc {
        delimiter,
        indented,
    });
    (heredoc, i)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoted_words_stay_whole() {
        let words = ShellWords::split(r#"gh issue comment 1 -b "two words""#).unwrap();

        assert_eq!(
            words.words,
            vec!["gh", "issue", "comment", "1", "-b", "two words"]
        );
        assert!(!words.has_heredoc);
    }

    #[test]
    fn a_heredoc_body_with_an_apostrophe_still_splits() {
        let command = "gh issue comment 1 --body-file - <<'EOF'\nit's fine\nEOF";

        let words = ShellWords::split(command).unwrap();

        assert_eq!(
            words.words,
            vec!["gh", "issue", "comment", "1", "--body-file", "-", "<<EOF"]
        );
        assert!(words.has_heredoc);
    }

    #[test]
    fn an_indented_heredoc_ends_at_its_indented_delimiter() {
        let command = "cat <<-EOF\n\tbody\n\tEOF\ngh issue view 1";

        let words = ShellWords::split(command).unwrap();

        assert!(words.words.ends_with(&[
            "gh".to_string(),
            "issue".into(),
            "view".into(),
            "1".into()
        ]));
    }

    #[test]
    fn a_here_string_is_not_a_heredoc() {
        let words = ShellWords::split("gh issue comment 1 --body-file - <<< 'text'").unwrap();

        assert!(!words.has_heredoc);
    }

    #[test]
    fn an_operator_attached_to_a_word_is_its_own_word() {
        let words = ShellWords::split("echo a; gh issue view 1|cat&& true").unwrap();

        assert_eq!(
            words.words,
            vec!["echo", "a", ";", "gh", "issue", "view", "1", "|", "cat", "&", "&", "true"]
        );
    }

    #[test]
    fn a_newline_separates_commands() {
        let words = ShellWords::split("echo a\ngh issue view 1").unwrap();

        assert_eq!(
            words.words,
            vec!["echo", "a", ";", "gh", "issue", "view", "1"]
        );
    }

    #[test]
    fn a_redirection_keeps_its_ampersand() {
        let words = ShellWords::split("gh issue view 1 2>&1 &>/dev/null").unwrap();

        assert_eq!(
            words.words,
            vec!["gh", "issue", "view", "1", "2>&1", "&>/dev/null"]
        );
    }

    #[test]
    fn an_operator_inside_quotes_is_text() {
        let words = ShellWords::split("gh issue comment 1 -b 'a; b' -t \"c|d\"").unwrap();

        assert_eq!(
            words.words,
            vec!["gh", "issue", "comment", "1", "-b", "a; b", "-t", "c|d"]
        );
    }

    #[test]
    fn unbalanced_quoting_outside_a_heredoc_does_not_parse() {
        assert_eq!(ShellWords::split("gh issue comment 1 -b \"unclosed"), None);
    }
}
