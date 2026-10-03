use std::fmt;
use std::path::Path;

use anyhow::{Context, Result};

use super::scanned_text::{ScannedText, TextOrigin};

/// One term that must never appear in anything posted to hub's public GitHub.
///
/// Trimmed, lowercased and non-empty. [`BannedTerm::parse`] is the only constructor, so
/// matching is case-insensitive by construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BannedTerm(String);

impl BannedTerm {
    /// A term from one line of the list, or `None` for a blank line.
    pub(crate) fn parse(line: &str) -> Option<Self> {
        let term = line.trim().to_lowercase();
        (!term.is_empty()).then_some(Self(term))
    }
}

impl fmt::Display for BannedTerm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The banned-terms list hub-private keeps beside this checkout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BannedTerms(Vec<BannedTerm>);

/// Whether this checkout has a list at all. Without hub-private there is nothing to ban.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum TermList {
    Absent,
    Present(BannedTerms),
}

/// A banned term found in text a call would post.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Hit {
    pub(crate) term: BannedTerm,
    pub(crate) origin: TextOrigin,
}

impl BannedTerms {
    /// Parses the list's contents, one term per line.
    pub(crate) fn parse(contents: &str) -> Self {
        Self(contents.lines().filter_map(BannedTerm::parse).collect())
    }

    /// Every banned term in every piece of text, in list order then text order.
    pub(crate) fn find_hits(&self, texts: &[ScannedText]) -> Vec<Hit> {
        let lowered: Vec<(&TextOrigin, String)> = texts
            .iter()
            .map(|scanned| (&scanned.origin, scanned.text.to_lowercase()))
            .collect();
        self.0
            .iter()
            .flat_map(|term| {
                lowered
                    .iter()
                    .filter(|(_, text)| text.contains(term.0.as_str()))
                    .map(|(origin, _)| Hit {
                        term: term.clone(),
                        origin: (*origin).clone(),
                    })
            })
            .collect()
    }
}

/// Reads the list at `path`.
///
/// # Errors
/// Returns an error when the list exists but cannot be read. A missing list is
/// [`TermList::Absent`], not an error.
pub(crate) fn load(path: &Path) -> Result<TermList> {
    match std::fs::read_to_string(path) {
        Ok(contents) => Ok(TermList::Present(BannedTerms::parse(&contents))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(TermList::Absent),
        Err(error) => Err(error)
            .with_context(|| format!("failed to read the banned-terms list at {}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command_text(text: &str) -> ScannedText {
        ScannedText {
            origin: TextOrigin::Command,
            text: text.to_string(),
        }
    }

    fn term(s: &str) -> BannedTerm {
        BannedTerm::parse(s).unwrap()
    }

    #[test]
    fn the_list_skips_blank_lines_and_folds_case() {
        let terms = BannedTerms::parse("Alpha\n\n  beta  \n\t\n");

        assert_eq!(terms, BannedTerms(vec![term("alpha"), term("beta")]));
    }

    #[test]
    fn a_blank_line_is_not_a_term() {
        assert_eq!(BannedTerm::parse("   "), None);
    }

    #[test]
    fn a_missing_list_means_nothing_is_banned() {
        let dir = tempfile::tempdir().unwrap();

        let list = load(&dir.path().join("blocked-terms.txt")).unwrap();

        assert_eq!(list, TermList::Absent);
    }

    #[test]
    fn a_list_that_exists_but_cannot_be_read_is_an_error() {
        let dir = tempfile::tempdir().unwrap();

        assert!(load(dir.path()).is_err());
    }

    #[test]
    fn a_present_list_is_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("blocked-terms.txt");
        std::fs::write(&path, "alpha\n").unwrap();

        let list = load(&path).unwrap();

        assert_eq!(list, TermList::Present(BannedTerms(vec![term("alpha")])));
    }

    #[test]
    fn a_term_is_found_whatever_its_case_in_the_text() {
        let terms = BannedTerms::parse("alpha");

        let hits = terms.find_hits(&[command_text("posting about ALPHA today")]);

        assert_eq!(
            hits,
            vec![Hit {
                term: term("alpha"),
                origin: TextOrigin::Command,
            }]
        );
    }

    #[test]
    fn a_hit_says_which_file_it_came_from() {
        let terms = BannedTerms::parse("alpha");
        let file = ScannedText {
            origin: TextOrigin::File("/tmp/body.md".into()),
            text: "alpha".to_string(),
        };

        let hits = terms.find_hits(&[command_text("clean"), file]);

        assert_eq!(
            hits.iter().map(|h| h.origin.clone()).collect::<Vec<_>>(),
            vec![TextOrigin::File("/tmp/body.md".into())]
        );
    }

    #[test]
    fn clean_text_has_no_hits() {
        let terms = BannedTerms::parse("alpha\nbeta");

        assert!(terms
            .find_hits(&[command_text("nothing to see")])
            .is_empty());
    }
}
