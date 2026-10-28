//! The label a project goes by where space is short: window and tab names.

use crate::investigation_window::{clamp, segment};
use crate::RepoSlug;

/// What a project is called in the windows and tabs hub opens for it.
///
/// Always lowercase letters, digits and single hyphens, at most
/// [`Self::MAX_LEN`] characters, which is exactly what a window's project
/// segment can hold. So a short name reaches a window unchanged.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ProjectShortName(String);

impl ProjectShortName {
    /// The longest short name, and so the longest project segment a window
    /// name has.
    pub const MAX_LEN: usize = 20;

    /// A short name somebody chose, refused unless it is already in the form
    /// a window shows, so that what was typed is what appears.
    ///
    /// # Errors
    /// Returns why `raw` is not a short name.
    pub fn parse(raw: &str) -> Result<Self, String> {
        let in_window_form = !raw.is_empty()
            && raw.len() <= Self::MAX_LEN
            && raw.split('-').all(|word| {
                !word.is_empty()
                    && word
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            });
        if in_window_form {
            Ok(Self(raw.to_string()))
        } else {
            Err(format!(
                "short_name {raw:?} must be lowercase letters, digits and single hyphens, at most {} characters",
                Self::MAX_LEN
            ))
        }
    }

    /// The short name of a project that chose none: its repository's name, as
    /// a window has always shown it.
    ///
    /// # Panics
    /// Panics if the derived name breaks the rules [`Self::parse`] enforces,
    /// which only a bug in the window's sanitising or shortening can cause.
    #[must_use]
    pub fn from_repo(repo: &RepoSlug) -> Self {
        let name = clamp(&segment(repo.repo_name()), Self::MAX_LEN);
        assert!(
            Self::parse(&name).is_ok(),
            "the short name {name:?} derived from {repo} breaks the rules a chosen one must meet"
        );
        Self(name)
    }

    /// The short name as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ProjectShortName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::ProjectShortName;
    use crate::RepoSlug;
    use proptest::prelude::*;
    use rstest::rstest;

    #[rstest]
    #[case("aoc")]
    #[case("site")]
    #[case("hub-private")]
    #[case("a1-b2-c3")]
    #[case("abcdefghij-klmnopqrs")]
    fn a_short_name_in_window_form_is_kept_as_written(#[case] raw: &str) {
        assert_eq!(ProjectShortName::parse(raw).unwrap().as_str(), raw);
    }

    #[rstest]
    #[case("", "empty")]
    #[case("Hub", "uppercase")]
    #[case("my app", "space")]
    #[case("-hub", "leading hyphen")]
    #[case("hub-", "trailing hyphen")]
    #[case("hub--private", "doubled hyphen")]
    #[case("hub.private", "punctuation a window would rewrite")]
    #[case("abcdefghij-klmnopqrst", "21 characters")]
    fn a_short_name_a_window_would_change_is_refused_naming_it(
        #[case] raw: &str,
        #[case] _reason: &str,
    ) {
        let error = ProjectShortName::parse(raw).unwrap_err();
        assert!(error.contains(&format!("{raw:?}")), "{error}");
    }

    fn slug() -> impl Strategy<Value = RepoSlug> {
        ("[A-Za-z0-9_-]{1,12}", "[A-Za-z0-9_. -]{1,40}")
            .prop_filter_map("not a slug", |(owner, repo)| {
                format!("{owner}/{repo}").parse().ok()
            })
    }

    proptest! {
        /// The fallback obeys the rules a chosen short name does, so the rest
        /// of hub never has to know which kind it holds.
        #[test]
        fn the_fallback_is_a_valid_short_name(repo in slug()) {
            let fallback = ProjectShortName::from_repo(&repo);
            prop_assert_eq!(ProjectShortName::parse(fallback.as_str()), Ok(fallback));
        }
    }
}
