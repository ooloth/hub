//! The project a window belongs to: what its name calls the project, and the repository whose
//! session its Rex tab joins.

use crate::{ProjectShortName, RepoSlug};

/// A project's short name and repository, kept together so that a window cannot take its name
/// from one project and its Rex session from another (decision 032).
///
/// Built once per project, from its `hub.toml` entry or, for a repository no project tracks, from
/// the short name a project tracking it would get.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowProject {
    short_name: ProjectShortName,
    repo: RepoSlug,
}

impl WindowProject {
    /// The project `short_name` names, whose repository is `repo`.
    #[must_use]
    pub const fn new(short_name: ProjectShortName, repo: RepoSlug) -> Self {
        Self { short_name, repo }
    }

    /// A repository no project tracks, named as a project tracking it would be.
    #[must_use]
    pub fn untracked(repo: &RepoSlug) -> Self {
        Self::new(ProjectShortName::from_repo(repo), repo.clone())
    }

    /// What window names call the project.
    #[must_use]
    pub const fn short_name(&self) -> &ProjectShortName {
        &self.short_name
    }

    /// The project's repository.
    #[must_use]
    pub const fn repo(&self) -> &RepoSlug {
        &self.repo
    }
}
