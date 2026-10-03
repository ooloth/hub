use std::fmt;
use std::path::{Path, PathBuf};

use super::publishing_call::{PublishKind, PublishingCall};
use super::shell_words::Word;

/// Where a publishing call would read text from, beyond the command line itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BodySource {
    /// A file, resolved against the command's working directory.
    File(PathBuf),
    /// Stdin fed by a heredoc, whose text is part of the command and scanned with it.
    Heredoc,
    /// A body the guard cannot read, so it cannot vouch for it.
    Unreadable {
        /// The argument exactly as written.
        arg: String,
        why: UnreadableReason,
    },
}

/// Why a body could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UnreadableReason {
    /// The shell would expand it (`$`, backticks, `~user`), and the guard sees it unexpanded.
    ShellExpansion,
    /// Stdin that no heredoc in the command supplies, such as a pipe.
    StdinWithoutHeredoc,
    /// No file at that path.
    NotFound,
    /// Larger than the guard reads.
    TooLarge,
    /// The file exists but reading it failed.
    CannotRead(String),
}

impl fmt::Display for UnreadableReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ShellExpansion => write!(
                f,
                "it uses shell expansion, which the guard sees unexpanded; write it literally"
            ),
            Self::StdinWithoutHeredoc => write!(
                f,
                "it reads stdin that no heredoc supplies; write the body to a file and pass its path"
            ),
            Self::NotFound => write!(f, "no file exists at that path"),
            Self::TooLarge => write!(f, "the file is larger than the guard reads"),
            Self::CannotRead(error) => write!(f, "reading it failed: {error}"),
        }
    }
}

impl BodySource {
    /// The bodies `call` would post besides its own command line.
    ///
    /// `home` resolves a leading `~/`. Inline text (`-b`, `-t`, `-f key=value`) is not
    /// returned: it is part of the command and scanned with it, unless it would be expanded.
    pub(crate) fn from_call(
        call: &PublishingCall,
        has_heredoc: bool,
        cwd: &Path,
        home: Option<&Path>,
    ) -> Vec<Self> {
        let resolve = |value: Value<'_>| resolve(value, has_heredoc, cwd, home);
        let mut sources = Vec::new();
        let mut args = call.args.iter().map(Value::from).peekable();

        while let Some(arg) = args.next() {
            let (flag, joined) = match arg.text.split_once('=') {
                Some((flag, value)) if flag.starts_with("--") => (
                    flag,
                    Some(Value {
                        text: value,
                        expands: arg.expands,
                    }),
                ),
                _ => (arg.text, None),
            };
            match (call.kind, flag) {
                (PublishKind::Api, "-F" | "--field") => {
                    if let Some(field) = joined.or_else(|| args.next()) {
                        sources.extend(api_field(field, has_heredoc, cwd, home));
                    }
                }
                (PublishKind::Api, "-f" | "--raw-field") => {
                    if let Some(field) = joined.or_else(|| args.next()) {
                        sources.extend(expansion(field));
                    }
                }
                (PublishKind::Api, "--input")
                | (PublishKind::IssueOrPr, "-F" | "--body-file")
                | (PublishKind::Release, "-F" | "--notes-file")
                | (PublishKind::GistEdit, "-a" | "--add") => {
                    if let Some(path) = joined.or_else(|| args.next()) {
                        sources.push(resolve(path));
                    }
                }
                // `pr review --comment` is a switch and `issue close --comment` takes text,
                // so the next word is text only when it is not another flag.
                (_, "-c" | "--comment") => {
                    let text = joined.or_else(|| args.next_if(|next| !next.text.starts_with('-')));
                    if let Some(text) = text {
                        sources.extend(expansion(text));
                    }
                }
                (
                    _,
                    "-b" | "--body" | "-t" | "--title" | "-n" | "--notes" | "-d" | "--desc"
                    | "--description",
                ) => {
                    if let Some(text) = joined.or_else(|| args.next()) {
                        sources.extend(expansion(text));
                    }
                }
                (PublishKind::Gist, "-f" | "--filename") => {
                    let _ = joined.or_else(|| args.next());
                }
                (PublishKind::Gist, positional)
                    if !positional.starts_with('-') || positional == "-" =>
                {
                    sources.push(resolve(arg));
                }
                _ => {}
            }
        }
        sources
    }
}

/// An argument's text, and whether the shell would expand it.
#[derive(Clone, Copy)]
struct Value<'a> {
    text: &'a str,
    expands: bool,
}

impl<'a> From<&'a Word> for Value<'a> {
    fn from(word: &'a Word) -> Self {
        Self {
            text: &word.text,
            expands: word.expands,
        }
    }
}

/// Where a path argument points, or why it cannot be read.
fn resolve(value: Value<'_>, has_heredoc: bool, cwd: &Path, home: Option<&Path>) -> BodySource {
    let arg = value.text;
    let unreadable = |why| BodySource::Unreadable {
        arg: arg.to_string(),
        why,
    };
    if arg == "-" {
        return if has_heredoc {
            BodySource::Heredoc
        } else {
            unreadable(UnreadableReason::StdinWithoutHeredoc)
        };
    }
    if value.expands {
        return unreadable(UnreadableReason::ShellExpansion);
    }
    match (arg.strip_prefix("~/"), home) {
        (Some(rest), Some(home)) => BodySource::File(home.join(rest)),
        (Some(_), None) => unreadable(UnreadableReason::ShellExpansion),
        (None, _) if arg.starts_with('~') => unreadable(UnreadableReason::ShellExpansion),
        (None, _) => BodySource::File(cwd.join(arg)),
    }
}

/// A `gh api -F key=value` field: `@path` reads a file, and anything else is inline text.
fn api_field(
    field: Value<'_>,
    has_heredoc: bool,
    cwd: &Path,
    home: Option<&Path>,
) -> Option<BodySource> {
    if field.expands {
        return expansion(field);
    }
    let (_, value) = field.text.split_once('=')?;
    let path = Value {
        text: value.strip_prefix('@')?,
        expands: false,
    };
    Some(match resolve(path, has_heredoc, cwd, home) {
        BodySource::Unreadable { why, .. } => BodySource::Unreadable {
            arg: field.text.to_string(),
            why,
        },
        readable => readable,
    })
}

/// Inline text is scanned with the command, unless the shell would replace it first.
fn expansion(value: Value<'_>) -> Option<BodySource> {
    value.expands.then(|| BodySource::Unreadable {
        arg: value.text.to_string(),
        why: UnreadableReason::ShellExpansion,
    })
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::super::shell_words::ShellWords;
    use super::*;

    fn sources(command: &str) -> Vec<BodySource> {
        let words = ShellWords::split(command).unwrap();
        PublishingCall::find_all(&words)
            .iter()
            .flat_map(|call| {
                BodySource::from_call(
                    call,
                    words.has_heredoc,
                    Path::new("/work"),
                    Some(Path::new("/home/me")),
                )
            })
            .collect()
    }

    fn file(path: &str) -> BodySource {
        BodySource::File(PathBuf::from(path))
    }

    fn unreadable(arg: &str, why: UnreadableReason) -> BodySource {
        BodySource::Unreadable {
            arg: arg.to_string(),
            why,
        }
    }

    #[rstest]
    #[case::body_file("gh issue comment 1 --body-file body.md", "/work/body.md")]
    #[case::body_file_joined("gh issue comment 1 --body-file=body.md", "/work/body.md")]
    #[case::short_flag("gh issue comment 1 -F body.md", "/work/body.md")]
    #[case::absolute("gh pr create -t t -F /tmp/body.md", "/tmp/body.md")]
    #[case::home("gh issue comment 1 --body-file ~/body.md", "/home/me/body.md")]
    #[case::release_notes("gh release create v1 --notes-file notes.md", "/work/notes.md")]
    #[case::api_field_file("gh api repos/o/r/issues -F body=@body.md", "/work/body.md")]
    #[case::api_input("gh api repos/o/r/issues --input req.json", "/work/req.json")]
    #[case::gist_file("gh gist create notes.md", "/work/notes.md")]
    #[case::line_continuation("gh issue comment 1 \\\n  --body-file body.md", "/work/body.md")]
    fn a_body_file_is_resolved_against_the_working_directory(
        #[case] command: &str,
        #[case] path: &str,
    ) {
        assert_eq!(sources(command), vec![file(path)]);
    }

    #[rstest]
    #[case::variable_path("gh issue comment 1 --body-file $S/body.md", "$S/body.md")]
    #[case::command_substitution(r#"gh issue comment 1 -b "$(cat x)""#, "$(cat x)")]
    #[case::backticks("gh issue comment 1 -b \"`cat x`\"", "`cat x`")]
    #[case::other_users_home("gh issue comment 1 --body-file ~other/b.md", "~other/b.md")]
    #[case::api_field_variable("gh api repos/o/r/issues -f body=$BODY", "body=$BODY")]
    #[case::double_quoted_variable(r#"gh issue comment 1 -b "hi $USER""#, "hi $USER")]
    #[case::mixed_quoting("gh issue comment 1 -b 'a'\"$B\"", "a$B")]
    fn a_body_the_shell_would_expand_is_unreadable(#[case] command: &str, #[case] arg: &str) {
        assert_eq!(
            sources(command),
            vec![unreadable(arg, UnreadableReason::ShellExpansion)]
        );
    }

    #[rstest]
    #[case::body_file("gh issue comment 1 --body-file -", "-")]
    #[case::api_field("gh api repos/o/r/issues -F body=@-", "body=@-")]
    fn stdin_without_a_heredoc_is_unreadable(#[case] command: &str, #[case] arg: &str) {
        assert_eq!(
            sources(command),
            vec![unreadable(arg, UnreadableReason::StdinWithoutHeredoc)]
        );
    }

    #[test]
    fn stdin_from_a_heredoc_is_readable() {
        assert_eq!(
            sources("gh issue comment 1 --body-file - <<'EOF'\nbody\nEOF"),
            vec![BodySource::Heredoc]
        );
    }

    #[rstest]
    #[case::inline_body("gh issue comment 1 -b 'plain text'")]
    #[case::title("gh pr create -t title -b body")]
    #[case::api_plain_field("gh api repos/o/r/issues -f title=plain -F number=3")]
    #[case::issue_number_variable("gh issue comment $N -b body")]
    #[case::single_quoted_backticks("gh issue create -t 'fix `daemon/` and $HOME' -b body")]
    #[case::single_quoted_api_field("gh api repos/o/r/issues -f 'title=costs $5 in `code`'")]
    #[case::escaped_dollar("gh issue comment 1 -b price\\$5")]
    #[case::escaped_inside_double_quotes(r#"gh issue comment 1 -b "costs \$5 and \`x\`""#)]
    fn inline_text_is_left_to_the_command_scan(#[case] command: &str) {
        assert!(sources(command).is_empty());
    }
}
