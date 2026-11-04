//! The Rex session a signal's tabs open in.

use crate::RepoSlug;

const WORKERS: &str = "Workers";

/// The label of the Rex session a signal's tabs open in: `Workers (<repo>)` for a repository's
/// tabs, or `Workers` for a signal with no repository.
///
/// `<repo>` is the part of the slug after the `/`. The dotfiles launcher labels its sessions from
/// the name a checkout's `origin` ends in, which for every repository hub clones from GitHub is
/// the same name, so hub's tabs and the launcher's tabs for one repository share a session
/// (decision 032).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkersSession(String);

impl WorkersSession {
    /// The session for tabs about `repo`.
    ///
    /// # Panics
    /// Panics if the slug has an empty repository name, which `RepoSlug` never allows.
    #[must_use]
    pub fn for_repo(repo: &RepoSlug) -> Self {
        let name = repo.repo_name();
        assert!(!name.is_empty(), "repository {repo} has an empty name");
        Self(format!("{WORKERS} ({name})"))
    }

    /// The session for tabs about a signal with no repository.
    #[must_use]
    pub fn unscoped() -> Self {
        Self(WORKERS.to_string())
    }

    /// The label as Rex shows and matches it.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for WorkersSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::WorkersSession;
    use crate::RepoSlug;
    use proptest::prelude::*;

    fn slug(raw: &str) -> RepoSlug {
        raw.parse().unwrap()
    }

    #[test]
    fn a_repositorys_tabs_open_in_the_session_named_after_it() {
        assert_eq!(
            WorkersSession::for_repo(&slug("ooloth/hub")).label(),
            "Workers (hub)"
        );
    }

    #[test]
    fn a_signal_with_no_repository_opens_in_plain_workers() {
        assert_eq!(WorkersSession::unscoped().label(), "Workers");
    }

    proptest! {
        /// The owner is never part of the label, because the launcher's labels name only the
        /// repository, and the name is kept whole, dots and dashes included.
        #[test]
        fn the_label_is_the_name_after_the_slash(
            owner in "[A-Za-z0-9-]{1,12}",
            name in "[A-Za-z0-9._-]{1,30}",
        ) {
            prop_assume!(name != "." && name != "..");
            let session = WorkersSession::for_repo(&slug(&format!("{owner}/{name}")));
            prop_assert_eq!(session.label(), format!("Workers ({name})"));
        }
    }
}
