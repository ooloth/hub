use std::fmt;
use std::path::Path;

use super::banned_terms::{BannedTerms, Hit};
use super::body_source::{BodySource, UnreadableReason};
use super::publishing_call::PublishingCall;
use super::scanned_text::{ScannedText, TextOrigin};
use super::shell_words::ShellWords;

/// The largest body file the guard reads. A real comment body is a few kilobytes.
pub(crate) const MAX_BODY_BYTES: u64 = 1024 * 1024;

/// Whether a Bash call may run.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Verdict {
    Allow,
    Deny(DenyReason),
}

/// Why a call was refused. Each variant keeps what is needed to find and fix the cause.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum DenyReason {
    /// The call would publish a banned term. Built from at least one hit, never zero.
    BannedTerm { first: Hit, rest: Vec<Hit> },
    /// The call would publish a body the guard cannot read.
    UnreadableBody { arg: String, why: UnreadableReason },
    /// The command mentions `gh` and its quoting does not parse.
    UnparseableCommand,
    /// The guard itself failed. It refuses rather than letting an unchecked post through.
    Internal(String),
}

impl fmt::Display for DenyReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BannedTerm { first, rest } => {
                write!(
                    f,
                    "This would publish a term banned from hub's public GitHub: "
                )?;
                for (i, hit) in std::iter::once(first).chain(rest).enumerate() {
                    if i > 0 {
                        write!(f, "; ")?;
                    }
                    write!(f, "\"{}\" in {}", hit.term, hit.origin)?;
                }
                write!(
                    f,
                    ". Replace it with a generic word, then run the command again. \
                     The list is hub-private/scripts/blocked-terms.txt."
                )
            }
            Self::UnreadableBody { arg, why } => write!(
                f,
                "Refusing to post: the guard cannot check the body `{arg}` because {why}."
            ),
            Self::UnparseableCommand => write!(
                f,
                "Refusing to run: this command calls gh and its quoting does not parse, so the \
                 guard cannot check what it would post. Put the body in a file and pass its path."
            ),
            Self::Internal(error) => write!(
                f,
                "Refusing to run: the gh post guard failed ({error}), so it cannot check what \
                 this would post."
            ),
        }
    }
}

/// Decides whether `command`, run in `cwd`, may run.
pub(crate) fn decide(
    command: &str,
    cwd: &Path,
    home: Option<&Path>,
    terms: &BannedTerms,
) -> Verdict {
    let Some(words) = ShellWords::split(command) else {
        let mentions_gh = command
            .split_whitespace()
            .any(|word| word == "gh" || word.ends_with("/gh"));
        return if mentions_gh {
            Verdict::Deny(DenyReason::UnparseableCommand)
        } else {
            Verdict::Allow
        };
    };

    let calls = PublishingCall::find_all(&words);
    if calls.is_empty() {
        return Verdict::Allow;
    }

    let mut texts = vec![ScannedText {
        origin: TextOrigin::Command,
        text: command.to_string(),
    }];
    let sources = calls
        .iter()
        .flat_map(|call| BodySource::from_call(call, words.has_heredoc, cwd, home));
    for source in sources {
        match source {
            BodySource::Heredoc => {}
            BodySource::Unreadable { arg, why } => {
                return Verdict::Deny(DenyReason::UnreadableBody { arg, why });
            }
            BodySource::File(path) => match read_body(&path) {
                Ok(text) => texts.push(ScannedText {
                    origin: TextOrigin::File(path),
                    text,
                }),
                Err(why) => {
                    return Verdict::Deny(DenyReason::UnreadableBody {
                        arg: path.display().to_string(),
                        why,
                    });
                }
            },
        }
    }

    let mut hits = terms.find_hits(&texts).into_iter();
    hits.next().map_or(Verdict::Allow, |first| {
        Verdict::Deny(DenyReason::BannedTerm {
            first,
            rest: hits.collect(),
        })
    })
}

/// A body file's text, if it exists and is small enough to read.
fn read_body(path: &Path) -> Result<String, UnreadableReason> {
    let metadata = std::fs::metadata(path).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => UnreadableReason::NotFound,
        _ => UnreadableReason::CannotRead(error.to_string()),
    })?;
    if metadata.len() > MAX_BODY_BYTES {
        return Err(UnreadableReason::TooLarge);
    }
    std::fs::read_to_string(path).map_err(|error| UnreadableReason::CannotRead(error.to_string()))
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use tempfile::TempDir;

    use super::super::scanned_text::TextOrigin;
    use super::*;

    fn terms() -> BannedTerms {
        BannedTerms::parse("alpha")
    }

    fn workdir_with(name: &str, contents: &str) -> TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(name), contents).unwrap();
        dir
    }

    fn verdict_in(dir: &TempDir, command: &str) -> Verdict {
        decide(command, dir.path(), None, &terms())
    }

    fn denied_term_origins(verdict: &Verdict) -> Vec<TextOrigin> {
        match verdict {
            Verdict::Deny(DenyReason::BannedTerm { first, rest }) => std::iter::once(first)
                .chain(rest)
                .map(|hit| hit.origin.clone())
                .collect(),
            other => panic!("expected a banned-term refusal, got {other:?}"),
        }
    }

    #[test]
    fn a_banned_term_in_a_body_file_is_refused_naming_the_file() {
        let dir = workdir_with("body.md", "notes about Alpha");

        let verdict = verdict_in(&dir, "gh issue comment 1 --body-file body.md");

        assert_eq!(
            denied_term_origins(&verdict),
            vec![TextOrigin::File(dir.path().join("body.md"))]
        );
    }

    #[rstest]
    #[case::inline_body("gh issue comment 1 -b 'about alpha'")]
    #[case::title("gh pr create -t 'Alpha fix' -b body")]
    #[case::heredoc("gh issue comment 1 --body-file - <<'EOF'\nit's alpha\nEOF")]
    #[case::api_field("gh api repos/o/r/issues -f body=ALPHA")]
    fn a_banned_term_in_the_command_is_refused(#[case] command: &str) {
        let dir = tempfile::tempdir().unwrap();

        let verdict = verdict_in(&dir, command);

        assert_eq!(denied_term_origins(&verdict), vec![TextOrigin::Command]);
    }

    #[test]
    fn the_refusal_names_the_term_and_where_it_was_found() {
        let dir = workdir_with("body.md", "alpha");

        let Verdict::Deny(reason) = verdict_in(&dir, "gh issue comment 1 --body-file body.md")
        else {
            panic!("expected a refusal")
        };

        let message = reason.to_string();
        assert!(message.contains("alpha"), "{message}");
        assert!(message.contains("body.md"), "{message}");
    }

    #[test]
    fn a_single_quoted_title_with_code_formatting_is_allowed() {
        let dir = workdir_with("body.md", "nothing banned here");

        let verdict = verdict_in(
            &dir,
            "gh issue create -t 'The tests fail when `daemon/` writes the cache' --body-file body.md",
        );

        assert_eq!(verdict, Verdict::Allow);
    }

    #[test]
    fn a_clean_post_is_allowed() {
        let dir = workdir_with("body.md", "nothing banned here");

        let verdict = verdict_in(&dir, "gh issue comment 1 -t clean --body-file body.md");

        assert_eq!(verdict, Verdict::Allow);
    }

    #[rstest]
    #[case::search("gh search issues alpha --repo o/r")]
    #[case::view("gh issue view 1 | grep -i alpha")]
    #[case::not_gh("rg -i alpha docs/")]
    fn a_read_mentioning_a_banned_term_is_allowed(#[case] command: &str) {
        let dir = tempfile::tempdir().unwrap();

        assert_eq!(verdict_in(&dir, command), Verdict::Allow);
    }

    #[test]
    fn a_missing_body_file_is_refused() {
        let dir = tempfile::tempdir().unwrap();

        let verdict = verdict_in(&dir, "gh issue comment 1 --body-file gone.md");

        assert_eq!(
            verdict,
            Verdict::Deny(DenyReason::UnreadableBody {
                arg: dir.path().join("gone.md").display().to_string(),
                why: UnreadableReason::NotFound,
            })
        );
    }

    #[test]
    fn an_oversized_body_file_is_refused() {
        let oversized = "x".repeat(usize::try_from(MAX_BODY_BYTES).unwrap() + 1);
        let dir = workdir_with("big.md", &oversized);

        let verdict = verdict_in(&dir, "gh issue comment 1 --body-file big.md");

        assert!(matches!(
            verdict,
            Verdict::Deny(DenyReason::UnreadableBody {
                why: UnreadableReason::TooLarge,
                ..
            })
        ));
    }

    #[test]
    fn a_body_behind_a_shell_variable_is_refused() {
        let dir = tempfile::tempdir().unwrap();

        let verdict = verdict_in(&dir, "gh issue comment 1 --body-file $S/body.md");

        assert_eq!(
            verdict,
            Verdict::Deny(DenyReason::UnreadableBody {
                arg: "$S/body.md".to_string(),
                why: UnreadableReason::ShellExpansion,
            })
        );
    }

    #[test]
    fn unparseable_quoting_around_gh_is_refused() {
        let dir = tempfile::tempdir().unwrap();

        let verdict = verdict_in(&dir, "gh issue comment 1 -b \"unclosed");

        assert_eq!(verdict, Verdict::Deny(DenyReason::UnparseableCommand));
    }

    #[test]
    fn unparseable_quoting_without_gh_is_allowed() {
        let dir = tempfile::tempdir().unwrap();

        assert_eq!(verdict_in(&dir, "echo \"unclosed"), Verdict::Allow);
    }
}
