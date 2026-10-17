//! Which pull request something is about.
use std::fmt;

use crate::pr::{PullRequest, RepoSlug};

/// A pull request, named by its repository and number.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PrId {
    /// The repository it is in.
    pub repo: RepoSlug,
    /// Its number in that repository.
    pub number: u64,
}

impl PrId {
    /// The pull request `pr` is.
    #[must_use]
    pub fn of(pr: &PullRequest) -> Self {
        Self {
            repo: pr.repo.clone(),
            number: pr.number,
        }
    }
}

/// `owner/repo#number`, the way GitHub writes a reference to one.
impl fmt::Display for PrId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}#{}", self.repo, self.number)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pull_request_reads_the_way_github_references_it() {
        let id = PrId {
            repo: RepoSlug::new("ooloth", "hub"),
            number: 344,
        };

        assert_eq!(id.to_string(), "ooloth/hub#344");
    }
}
