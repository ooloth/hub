use anyhow::{anyhow, bail, Result};
use domain::{ProjectShortName, RepoSlug};
use std::collections::HashMap;

use crate::toml;

/// A project from a `[[project]]` entry in `hub.toml`, with every label settled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    /// What the TUI calls the project. The repository's name unless `name` is set.
    pub name: String,
    /// What window and tab names call the project. The repository's name, as a
    /// window has always shown it, unless `short_name` is set.
    pub short_name: ProjectShortName,
    /// The project's GitHub repository, which also locates its clone.
    pub repo: RepoSlug,
    /// Workflows enabled at the repo level (e.g. PRs, CI).
    pub workflow: Vec<toml::WorkflowConfig>,
    /// Per-environment configurations (e.g. prod, staging).
    pub environment: Vec<toml::Environment>,
}

impl Project {
    /// Settles the labels `raw` left out, and refuses the ones it set badly.
    ///
    /// # Errors
    /// Returns an error naming the project's repository if `name` is blank or
    /// `short_name` is not in the form a window shows.
    pub(crate) fn resolve(raw: toml::Project) -> Result<Self> {
        let repo = raw.repo;
        let name = match raw.name {
            Some(name) if name.trim().is_empty() => {
                bail!(
                    "project {repo}: name must not be blank; leave it out to use {:?}",
                    repo.repo_name()
                )
            }
            Some(name) => name,
            None => repo.repo_name().to_string(),
        };
        let short_name = match raw.short_name {
            Some(chosen) => ProjectShortName::parse(&chosen)
                .map_err(|reason| anyhow!("project {repo}: {reason}"))?,
            None => ProjectShortName::from_repo(&repo),
        };
        Ok(Self {
            name,
            short_name,
            repo,
            workflow: raw.workflow,
            environment: raw.environment,
        })
    }
}

/// Every configured project, no two sharing a short name.
///
/// A window is found again by building its name, and that name starts with the
/// project's short name, so two projects sharing one would open each other's
/// windows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Projects(Vec<Project>);

impl Projects {
    /// Resolves every `[[project]]` entry and checks the set as a whole.
    ///
    /// # Errors
    /// Returns the first project whose labels are refused, or the first two
    /// projects found sharing a short name.
    pub(crate) fn resolve(raw: Vec<toml::Project>) -> Result<Self> {
        let projects = raw
            .into_iter()
            .map(Project::resolve)
            .collect::<Result<Vec<_>>>()?;
        let mut seen = HashMap::new();
        for project in &projects {
            if let Some(earlier) = seen.insert(&project.short_name, &project.repo) {
                bail!(
                    "projects {earlier} and {} share the short_name {:?}; give one of them a different short_name",
                    project.repo,
                    project.short_name.as_str()
                );
            }
        }
        Ok(Self(projects))
    }

    /// The project tracking `repo`, which is what a launch needs before it can
    /// have a clone to work in.
    ///
    /// # Errors
    /// Returns an error naming `repo` if no `[[project]]` tracks it. A
    /// repository sharing a tracked one's name is not tracked by it.
    pub fn tracked(&self, repo: &RepoSlug) -> Result<&Project> {
        self.0
            .iter()
            .find(|project| &project.repo == repo)
            .ok_or_else(|| {
                anyhow!(
                    "{repo} is not a [[project]] in hub.toml, so hub has no clone of it to work in"
                )
            })
    }

    /// What windows for `repo` are called: its project's short name, or, for
    /// a repository no project tracks, the name a project tracking it would
    /// get by default.
    #[must_use]
    pub fn short_name_for(&self, repo: &RepoSlug) -> ProjectShortName {
        self.tracked(repo).map_or_else(
            |_| ProjectShortName::from_repo(repo),
            |project| project.short_name.clone(),
        )
    }

    /// The projects, in the order `hub.toml` lists them.
    #[must_use]
    pub fn as_slice(&self) -> &[Project] {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::{Project, Projects};
    use crate::toml;
    use rstest::rstest;

    fn raw(repo: &str, name: Option<&str>, short_name: Option<&str>) -> toml::Project {
        toml::Project {
            name: name.map(Into::into),
            short_name: short_name.map(Into::into),
            repo: repo.parse().unwrap(),
            workflow: vec![],
            environment: vec![],
        }
    }

    #[test]
    fn a_project_that_names_nothing_goes_by_its_repository_name() {
        let project = Project::resolve(raw("ooloth/michaeluloth.com", None, None)).unwrap();

        assert_eq!(project.name, "michaeluloth.com");
        assert_eq!(project.short_name.as_str(), "michaeluloth-com");
    }

    #[test]
    fn labels_a_project_sets_are_kept_as_written() {
        let project = Project::resolve(raw(
            "ooloth/michaeluloth.com",
            Some("My Site"),
            Some("site"),
        ))
        .unwrap();

        assert_eq!(project.name, "My Site");
        assert_eq!(project.short_name.as_str(), "site");
    }

    #[test]
    fn a_short_name_a_window_would_change_is_refused_naming_the_project() {
        let error = Project::resolve(raw("ooloth/hub", None, Some("Hub")))
            .unwrap_err()
            .to_string();

        assert!(error.contains("ooloth/hub"), "{error}");
        assert!(error.contains(r#""Hub""#), "{error}");
    }

    #[rstest]
    #[case("")]
    #[case("   ")]
    fn a_blank_name_is_refused_naming_the_project(#[case] name: &str) {
        let error = Project::resolve(raw("ooloth/hub", Some(name), None))
            .unwrap_err()
            .to_string();

        assert!(error.contains("ooloth/hub"), "{error}");
        assert!(error.contains("name"), "{error}");
    }

    #[rstest]
    #[case::both_chosen(
        raw("ooloth/hub", None, Some("h")),
        raw("ooloth/home", None, Some("h"))
    )]
    #[case::chosen_and_fallback(
        raw("ooloth/site", None, Some("hub")),
        raw("ooloth/hub", None, None)
    )]
    #[case::both_fallback(raw("ooloth/hub", None, None), raw("work/hub", None, None))]
    fn two_projects_sharing_a_short_name_are_refused_naming_both(
        #[case] first: toml::Project,
        #[case] second: toml::Project,
    ) {
        let (a, b) = (first.repo.to_string(), second.repo.to_string());

        let error = Projects::resolve(vec![first, second])
            .unwrap_err()
            .to_string();

        assert!(error.contains(&a), "{error}");
        assert!(error.contains(&b), "{error}");
    }

    #[test]
    fn a_tracked_repo_has_its_project() {
        let projects = Projects::resolve(vec![raw("ooloth/hub", None, Some("h"))]).unwrap();

        let project = projects.tracked(&"ooloth/hub".parse().unwrap()).unwrap();

        assert_eq!(project.short_name.as_str(), "h");
    }

    /// A review request can come from a repository that shares a tracked one's
    /// name, and launching it in the tracked clone would investigate the wrong
    /// repository's pull request.
    #[test]
    fn a_repo_sharing_a_tracked_repos_name_is_not_tracked() {
        let projects = Projects::resolve(vec![raw("ooloth/hub", None, Some("h"))]).unwrap();

        let error = projects
            .tracked(&"someone/hub".parse().unwrap())
            .unwrap_err()
            .to_string();

        assert!(error.contains("someone/hub"), "{error}");
        assert!(error.contains("[[project]]"), "{error}");
    }

    #[test]
    fn a_tracked_repo_is_called_by_its_projects_short_name() {
        let projects = Projects::resolve(vec![raw("ooloth/hub", None, Some("h"))]).unwrap();

        assert_eq!(
            projects
                .short_name_for(&"ooloth/hub".parse().unwrap())
                .as_str(),
            "h"
        );
    }

    /// A review request can come from a repository hub does not track.
    #[test]
    fn an_untracked_repo_is_called_what_a_project_tracking_it_would_be_by_default() {
        let projects = Projects::resolve(vec![raw("ooloth/hub", None, Some("h"))]).unwrap();
        let other: domain::RepoSlug = "work/client-operations-site".parse().unwrap();

        assert_eq!(
            projects.short_name_for(&other),
            domain::ProjectShortName::from_repo(&other)
        );
    }

    #[test]
    fn projects_with_distinct_short_names_keep_their_order() {
        let projects = Projects::resolve(vec![
            raw("ooloth/hub", None, None),
            raw("ooloth/hub-private", None, Some("hubp")),
        ])
        .unwrap();

        let names: Vec<&str> = projects
            .as_slice()
            .iter()
            .map(|p| p.short_name.as_str())
            .collect();
        assert_eq!(names, ["hub", "hubp"]);
    }
}
