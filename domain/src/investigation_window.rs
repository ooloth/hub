//! Deterministic names for the windows hub opens, in tmux or Rex.

use crate::window_segment::{clamp, segment, short_hash};
use crate::{
    GitHubLogin, PrWindowActivity, ProjectShortName, RexLabel, WindowProject, WorkersSession,
};

/// Longest window name hub will produce. Beyond this the tmux status bar or
/// Rex tab bar crowds out everything else.
const MAX_LEN: usize = 40;

/// Budgets for the three segments. They sum, with the two separators, to
/// exactly `MAX_LEN`, which is what makes the length postcondition in `build`
/// a check on this code rather than a trap for long external names.
///
/// The project budget is set so that real repository names arrive intact:
/// `michaeluloth-com` and `advent-of-code-2015` both fit, and shortening is
/// the rare fallback rather than the common case.
const MAX_PROJECT: usize = crate::ProjectShortName::MAX_LEN;
const MAX_KIND: usize = 5;
const MAX_DISCRIMINATOR: usize = 13;

/// Length of the hash that *is* an alert's identity. Every distinct alert
/// depends on this one, so it gets a wider space than a truncation tie-break.
const IDENTITY_HASH_LEN: usize = 6;

/// Which log source an alert came from, for the window's kind segment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AlertSource {
    /// A Loki log alert.
    Loki,
    /// A Google Cloud Logging alert.
    Gcp,
}

impl AlertSource {
    const fn kind(self) -> &'static str {
        match self {
            Self::Loki => "loki",
            Self::Gcp => "gcp",
        }
    }

    /// The word a Rex label starts with.
    const fn label(self) -> &'static str {
        match self {
            Self::Loki => "Loki",
            Self::Gcp => "GCP",
        }
    }
}

/// The name of a window hub opened, in tmux or Rex, addressable afterwards.
///
/// Every name is `<project>:<kind>:<discriminator>`, built through one
/// constructor per signal kind. The project segment is always a
/// [`ProjectShortName`], so a configured `short_name` cannot be bypassed.
///
/// The kind segment comes from the constructor rather than the caller. It is
/// what keeps the windows one pull request can have apart in tmux: its
/// investigation, lazygit, Octo and diff windows share a project and a number
/// and differ only in kind. Its investigation and reviews share the kind `pr`,
/// and so one tmux window.
///
/// Construction is deterministic in its inputs. That is the property that lets
/// a later launch find the window an earlier one opened, by building the same
/// name again rather than by recording it anywhere.
///
/// Each window also carries what Rex needs: the session its tab belongs in,
/// built from the same [`WindowProject`] as the name (decision 032), and the
/// tab's label, by which Rex finds it again (decision 033). In Rex each review
/// skill has its own label, and so its own tab. tmux ignores both.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvestigationWindow {
    name: String,
    session: WorkersSession,
    label: RexLabel,
}

impl InvestigationWindow {
    /// Window for one activity on a pull request `author` opened.
    ///
    /// # Panics
    /// Panics if `number` is zero.
    #[must_use]
    pub fn pr(
        project: &WindowProject,
        number: u64,
        author: &GitHubLogin,
        activity: PrWindowActivity,
    ) -> Self {
        Self::numbered(
            project,
            activity.tmux_kind(),
            number,
            RexLabel::pr(number, author, activity.label()),
        )
    }

    /// Window for an investigation of a GitHub issue `author` opened.
    ///
    /// # Panics
    /// Panics if `number` is zero.
    #[must_use]
    pub fn issue(project: &WindowProject, number: u64, author: &GitHubLogin) -> Self {
        Self::numbered(project, "issue", number, RexLabel::issue(number, author))
    }

    /// Window for an investigation of a failing CI workflow.
    ///
    /// Named by workflow rather than by run, so re-running a failing workflow
    /// maps to the window already investigating it.
    #[must_use]
    pub fn ci(project: &WindowProject, workflow: &str) -> Self {
        Self::for_project(project, "ci", &segment(workflow), RexLabel::ci(workflow))
    }

    /// Window for an investigation of a log alert.
    ///
    /// `message` is the stable error category, not the human-readable query
    /// title, which several distinct alerts share. Scoped by `env`, since the
    /// same error in two environments is two investigations.
    #[must_use]
    pub fn alert(project: &WindowProject, source: AlertSource, env: &str, message: &str) -> Self {
        // Uniqueness rests entirely on the hash, which covers the environment
        // as well as the message. The readable environment prefix is
        // decoration, so shortening it cannot make two alerts collide.
        let mut readable = segment(env);
        readable.truncate(MAX_DISCRIMINATOR - IDENTITY_HASH_LEN - 1);
        let discriminator = format!(
            "{}-{}",
            readable.trim_end_matches('-'),
            short_hash(&format!("{env}\u{1}{message}"), IDENTITY_HASH_LEN),
        );
        Self::for_project(
            project,
            source.kind(),
            &discriminator,
            RexLabel::alert(source.label(), env, message),
        )
    }

    /// Window for an investigation of a blocked media item.
    ///
    /// These signals carry no repository or number, so the item's title is the
    /// only thing distinguishing one from another.
    #[must_use]
    pub fn media(title: &str) -> Self {
        Self::build(
            "media",
            "item",
            &segment(title),
            WorkersSession::unscoped(),
            RexLabel::media(title),
        )
    }

    fn numbered(project: &WindowProject, kind: &str, number: u64, label: RexLabel) -> Self {
        assert!(number > 0, "signal number must be positive, got {number}");
        Self::for_project(project, kind, &number.to_string(), label)
    }

    /// A window named for `project`, whose short name already fits the
    /// project segment and so must arrive in it unchanged, with its tab in
    /// the project's repository's session.
    fn for_project(
        project: &WindowProject,
        kind: &str,
        discriminator: &str,
        label: RexLabel,
    ) -> Self {
        let short_name: &ProjectShortName = project.short_name();
        let window = Self::build(
            short_name.as_str(),
            kind,
            discriminator,
            WorkersSession::for_repo(project.repo()),
            label,
        );
        assert_eq!(
            window.name.split(':').next(),
            Some(short_name.as_str()),
            "window {window} does not start with the short name {short_name}"
        );
        window
    }

    fn build(
        project: &str,
        kind: &str,
        discriminator: &str,
        session: WorkersSession,
        label: RexLabel,
    ) -> Self {
        assert!(
            kind.len() <= MAX_KIND,
            "kind {kind} exceeds {MAX_KIND} characters"
        );

        // Each segment is clamped separately rather than the joined name, so
        // that a long project cannot truncate the kind away and leave a name
        // that says nothing about what it is. MAX_PROJECT + MAX_KIND +
        // MAX_DISCRIMINATOR plus two separators is MAX_LEN, so the length
        // postcondition below cannot fire on external input however long.
        let name = format!(
            "{}:{kind}:{}",
            clamp(&segment(project), MAX_PROJECT),
            clamp(discriminator, MAX_DISCRIMINATOR),
        );

        assert!(!name.is_empty(), "window name must not be empty");
        assert!(
            name.len() <= MAX_LEN,
            "window name {name} exceeds {MAX_LEN} characters"
        );
        assert!(
            !name.contains(char::is_whitespace),
            "window name {name} contains whitespace"
        );

        Self {
            name,
            session,
            label,
        }
    }

    /// The Rex session this window's tab belongs in.
    #[must_use]
    pub const fn session(&self) -> &WorkersSession {
        &self.session
    }

    /// The label this window's Rex tab carries.
    #[must_use]
    pub const fn label(&self) -> &RexLabel {
        &self.label
    }
}

impl std::fmt::Display for InvestigationWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)
    }
}

#[cfg(test)]
mod tests {
    use super::{AlertSource, InvestigationWindow, MAX_LEN};
    use crate::{
        GitHubLogin, PrWindowActivity, ProjectShortName, RepoSlug, ReviewSkill, WindowProject,
    };
    use proptest::prelude::*;
    use rstest::rstest;

    fn alice() -> GitHubLogin {
        GitHubLogin::parse("alice").unwrap()
    }

    fn project(short: &ProjectShortName, repo: &RepoSlug) -> WindowProject {
        WindowProject::new(short.clone(), repo.clone())
    }

    fn on_pr(
        short: &ProjectShortName,
        repo: &RepoSlug,
        number: u64,
        activity: PrWindowActivity,
    ) -> InvestigationWindow {
        InvestigationWindow::pr(&project(short, repo), number, &alice(), activity)
    }

    fn pr(short: &ProjectShortName, repo: &RepoSlug, number: u64) -> InvestigationWindow {
        on_pr(short, repo, number, PrWindowActivity::Investigate)
    }

    fn lazygit(short: &ProjectShortName, repo: &RepoSlug, number: u64) -> InvestigationWindow {
        on_pr(short, repo, number, PrWindowActivity::Lazygit)
    }

    fn octo(short: &ProjectShortName, repo: &RepoSlug, number: u64) -> InvestigationWindow {
        on_pr(short, repo, number, PrWindowActivity::Octo)
    }

    fn diff(short: &ProjectShortName, repo: &RepoSlug, number: u64) -> InvestigationWindow {
        on_pr(short, repo, number, PrWindowActivity::Diff)
    }

    fn issue(short: &ProjectShortName, repo: &RepoSlug, number: u64) -> InvestigationWindow {
        InvestigationWindow::issue(&project(short, repo), number, &alice())
    }

    fn ci(short: &ProjectShortName, repo: &RepoSlug, workflow: &str) -> InvestigationWindow {
        InvestigationWindow::ci(&project(short, repo), workflow)
    }

    fn alert(
        short: &ProjectShortName,
        repo: &RepoSlug,
        source: AlertSource,
        env: &str,
        message: &str,
    ) -> InvestigationWindow {
        InvestigationWindow::alert(&project(short, repo), source, env, message)
    }

    /// The short name a project tracking `repo` gets when it chooses none.
    fn of(repo: &str) -> ProjectShortName {
        ProjectShortName::from_repo(&repo.parse().unwrap())
    }

    fn slug(repo: &str) -> RepoSlug {
        repo.parse().unwrap()
    }

    fn hub_repo() -> RepoSlug {
        slug("ooloth/hub")
    }

    fn short(name: &str) -> ProjectShortName {
        ProjectShortName::parse(name).unwrap()
    }

    fn short_names() -> impl Strategy<Value = ProjectShortName> {
        "[a-z0-9]{1,8}(-[a-z0-9]{1,4}){0,3}"
            .prop_filter_map("too long", |raw| ProjectShortName::parse(&raw).ok())
    }

    fn assert_well_formed(name: &str) {
        assert!(!name.is_empty(), "empty name");
        assert!(name.len() <= MAX_LEN, "{name} is longer than {MAX_LEN}");
        assert!(!name.contains(char::is_whitespace), "{name} has whitespace");
    }

    proptest! {
        /// The three postconditions hold for short names the author did not
        /// imagine. Each case also exercises the assertions inside `build`.
        #[test]
        fn pr_window_is_well_formed_for_any_short_name(
            project in short_names(),
            number in 1u64..=u64::MAX,
        ) {
            assert_well_formed(&pr(&project, &hub_repo(), number).to_string());
        }

        /// A short name is chosen to be what the window shows, so the window
        /// must not rewrite or shorten it.
        #[test]
        fn the_project_segment_is_the_short_name_unchanged(
            project in short_names(),
            number in 1u64..=u64::MAX,
        ) {
            let name = pr(&project, &hub_repo(), number).to_string();
            prop_assert_eq!(name.split(':').next(), Some(project.as_str()));
        }

        /// Workflow names are the least constrained input: GitHub allows
        /// punctuation, emoji and considerable length.
        #[test]
        fn ci_window_is_well_formed_for_any_workflow(project in short_names(), workflow in ".*") {
            assert_well_formed(&ci(&project, &hub_repo(), &workflow).to_string());
        }

        #[test]
        fn alert_window_is_well_formed_for_any_message(
            project in short_names(),
            env in ".*",
            message in ".*",
        ) {
            let window = alert(&project, &hub_repo(), AlertSource::Loki, &env, &message);
            assert_well_formed(&window.to_string());
        }

        /// #331 finds an open window by building its name again, so equal
        /// inputs must always give equal names.
        #[test]
        fn construction_is_deterministic(project in short_names(), workflow in ".*") {
            prop_assert_eq!(
                ci(&project, &hub_repo(), &workflow),
                ci(&project, &hub_repo(), &workflow),
            );
        }

        /// Two alerts must be investigable side by side. Distinctness rests on
        /// a truncated hash, so this holds up to a hash collision.
        #[test]
        fn distinct_alert_messages_get_distinct_windows(a in ".*", b in ".*") {
            prop_assume!(a != b);
            prop_assert_ne!(
                alert(&short("api"), &hub_repo(), AlertSource::Loki, "prod", &a),
                alert(&short("api"), &hub_repo(), AlertSource::Loki, "prod", &b),
            );
        }

        /// A shortened segment must not leave a separator butting against the
        /// hash that follows it. The generator is biased towards long,
        /// separator-heavy names, since those are the only ones that truncate.
        #[test]
        fn no_window_name_has_a_doubled_separator(
            project in short_names(),
            workflow in "[a-z]{1,4}([ /._-][a-z]{1,4}){0,12}",
        ) {
            let name = ci(&project, &hub_repo(), &workflow).to_string();
            prop_assert!(!name.contains("--"), "{} has a doubled separator", name);
        }

        /// The same error in two environments is two investigations.
        #[test]
        fn distinct_alert_environments_get_distinct_windows(a in ".*", b in ".*") {
            prop_assume!(a != b);
            prop_assert_ne!(
                alert(&short("api"), &hub_repo(), AlertSource::Loki, &a, "boom"),
                alert(&short("api"), &hub_repo(), AlertSource::Loki, &b, "boom"),
            );
        }
    }

    proptest! {
        /// One pull request can have four windows open at once, and an issue
        /// window has the same shape, so the kind alone must tell them apart.
        #[test]
        fn every_window_kind_for_one_number_has_its_own_name(
            project in short_names(),
            number in 1u64..=u64::MAX,
        ) {
            let names: std::collections::HashSet<String> = [
                pr(&project, &hub_repo(), number),
                issue(&project, &hub_repo(), number),
                lazygit(&project, &hub_repo(), number),
                octo(&project, &hub_repo(), number),
                diff(&project, &hub_repo(), number),
            ]
            .iter()
            .map(ToString::to_string)
            .collect();
            prop_assert_eq!(names.len(), 5);
        }
    }

    #[rstest]
    #[case(pr(&of("ooloth/hub"), &slug("ooloth/hub"), 330), "hub:pr:330")]
    #[case(issue(&of("ooloth/hub"), &slug("ooloth/hub"), 412), "hub:issue:412")]
    #[case(lazygit(&of("ooloth/hub"), &slug("ooloth/hub"), 330), "hub:git:330")]
    #[case(octo(&of("ooloth/hub"), &slug("ooloth/hub"), 330), "hub:octo:330")]
    #[case(diff(&of("ooloth/hub"), &slug("ooloth/hub"), 330), "hub:diff:330")]
    fn the_owner_is_dropped_and_the_kind_names_the_window(
        #[case] window: InvestigationWindow,
        #[case] expected: &str,
    ) {
        assert_eq!(window.to_string(), expected);
    }

    /// A chosen short name is the whole point: it replaces the repository's.
    #[test]
    fn a_chosen_short_name_names_the_window() {
        assert_eq!(pr(&short("h"), &hub_repo(), 7).to_string(), "h:pr:7");
    }

    /// A slash inside a workflow name is punctuation, not an owner prefix.
    #[rstest]
    #[case("CI / build (push)", "hub:ci:ci-build-push")]
    #[case("lint", "hub:ci:lint")]
    #[case("Test   Suite", "hub:ci:test-suite")]
    fn workflow_punctuation_becomes_a_single_separator(
        #[case] workflow: &str,
        #[case] expected: &str,
    ) {
        assert_eq!(
            ci(&short("hub"), &hub_repo(), workflow).to_string(),
            expected
        );
    }

    /// External text that sanitises to nothing must not produce an empty name.
    #[test]
    fn a_workflow_named_only_with_punctuation_falls_back() {
        let name = ci(&short("hub"), &hub_repo(), "!!! ///").to_string();
        assert_eq!(name, "hub:ci:unnamed");
    }

    /// A long project must not truncate the kind away, or the window stops
    /// saying what it is.
    #[test]
    fn a_long_project_keeps_its_kind_and_discriminator() {
        let name = pr(
            &of("ooloth/a-very-long-repository-name"),
            &slug("ooloth/a-very-long-repository-name"),
            330,
        )
        .to_string();
        assert!(name.ends_with(":pr:330"), "{name} lost its kind or number");
        assert!(name.len() <= MAX_LEN, "{name} is too long");
    }

    /// A shortened repository still has to be recognisable at a glance, so the
    /// disambiguating hash is kept short enough to leave readable characters.
    #[test]
    fn a_shortened_repository_stays_recognisable() {
        let name = pr(
            &of("ooloth/a-very-long-repository-name"),
            &slug("ooloth/a-very-long-repository-name"),
            330,
        )
        .to_string();
        assert!(
            name.starts_with("a-very-long-rep"),
            "{name} is not recognisable"
        );
    }

    /// The budget exists so that real repository names are never shortened.
    #[rstest]
    #[case("ooloth/michaeluloth.com", "michaeluloth-com:pr:12")]
    #[case("ooloth/advent-of-code-2015", "advent-of-code-2015:pr:12")]
    #[case("ooloth/hub-private", "hub-private:pr:12")]
    #[case("ooloth/config.nvim", "config-nvim:pr:12")]
    fn real_repository_names_survive_intact(#[case] repo: &str, #[case] expected: &str) {
        assert_eq!(pr(&of(repo), &slug(repo), 12).to_string(), expected);
    }

    /// The case that surfaced this: the kept prefix ends on a separator, and
    /// the hash separator would double it.
    #[test]
    fn a_prefix_ending_in_a_separator_does_not_double_it() {
        let name = ci(
            &short("hub"),
            &hub_repo(),
            "build and test everything nightly",
        )
        .to_string();
        assert!(!name.contains("--"), "{name} has a doubled separator");
    }

    /// Truncation must not be able to manufacture a collision. The names are compared, not the
    /// windows, because the two windows' sessions differ whatever their names are.
    #[test]
    fn two_long_projects_sharing_a_prefix_get_different_window_names() {
        assert_ne!(
            pr(
                &of("ooloth/a-very-long-repository-name-one"),
                &slug("ooloth/a-very-long-repository-name-one"),
                1
            )
            .to_string(),
            pr(
                &of("ooloth/a-very-long-repository-name-two"),
                &slug("ooloth/a-very-long-repository-name-two"),
                1
            )
            .to_string(),
        );
    }

    /// Media signals have no repository or number, so two blocked items must
    /// still be told apart by title alone.
    #[test]
    fn two_blocked_media_items_get_different_windows() {
        assert_ne!(
            InvestigationWindow::media("Some Show - S01E01"),
            InvestigationWindow::media("Some Show - S01E02"),
        );
    }

    /// Every kind of window for one repository puts its tab in that repository's session, so a
    /// lookup never searches another repository's tabs.
    #[rstest]
    #[case(pr(&short("hub"), &hub_repo(), 330))]
    #[case(issue(&short("hub"), &hub_repo(), 412))]
    #[case(ci(&short("hub"), &hub_repo(), "lint"))]
    #[case(alert(&short("hub"), &hub_repo(), AlertSource::Gcp, "prod", "boom"))]
    #[case(lazygit(&short("hub"), &hub_repo(), 330))]
    #[case(octo(&short("hub"), &hub_repo(), 330))]
    #[case(diff(&short("hub"), &hub_repo(), 330))]
    fn a_repositorys_windows_open_in_its_session(#[case] window: InvestigationWindow) {
        assert_eq!(window.session().label(), "Workers (hub)");
    }

    /// The session is named after the repository, not the short name, so it matches the session
    /// the dotfiles launcher picks for the same repository.
    #[test]
    fn a_chosen_short_name_does_not_rename_the_session() {
        let window = pr(&short("h"), &hub_repo(), 7);
        assert_eq!(window.to_string(), "h:pr:7");
        assert_eq!(window.session().label(), "Workers (hub)");
    }

    /// Media signals carry no repository, so their tabs have no repository's session to join.
    #[test]
    fn a_media_window_opens_in_plain_workers() {
        assert_eq!(
            InvestigationWindow::media("Some Show - S01E01")
                .session()
                .label(),
            "Workers"
        );
    }

    #[test]
    #[should_panic(expected = "signal number must be positive")]
    fn a_zero_signal_number_is_a_programmer_error() {
        let _ = pr(&of("ooloth/hub"), &slug("ooloth/hub"), 0);
    }

    /// Every kind of tab has the label decision 033 gives it.
    #[rstest]
    #[case(pr(&short("hub"), &hub_repo(), 12), "PR 12 (alice): investigate")]
    #[case(lazygit(&short("hub"), &hub_repo(), 12), "PR 12 (alice): lazygit")]
    #[case(octo(&short("hub"), &hub_repo(), 12), "PR 12 (alice): octo")]
    #[case(diff(&short("hub"), &hub_repo(), 12), "PR 12 (alice): diff")]
    #[case(issue(&short("hub"), &hub_repo(), 412), "Issue 412 (alice): investigate")]
    #[case(ci(&short("hub"), &hub_repo(), "test-suite"), "CI test-suite: investigate")]
    #[case(alert(&short("hub"), &hub_repo(), AlertSource::Loki, "prod", "boom"), "Loki prod boom: investigate")]
    #[case(alert(&short("hub"), &hub_repo(), AlertSource::Gcp, "prod", "boom"), "GCP prod boom: investigate")]
    #[case(
        InvestigationWindow::media("Some Show"),
        "Media Some Show: investigate"
    )]
    fn each_tab_is_labelled_as_decision_033_says(
        #[case] window: InvestigationWindow,
        #[case] expected: &str,
    ) {
        assert_eq!(window.label().as_str(), expected);
    }

    /// `monitor-prs` counts a review only when the label names the skill the tab runs.
    #[rstest]
    #[case(ReviewSkill::ReviewCode, "PR 12 (alice): review-code")]
    #[case(ReviewSkill::ReviewConverge, "PR 12 (alice): review-converge")]
    #[case(
        ReviewSkill::ReviewPrCommentsConverge,
        "PR 12 (alice): review-pr-comments-converge"
    )]
    fn a_review_tab_names_its_skill(#[case] skill: ReviewSkill, #[case] expected: &str) {
        let window = on_pr(
            &short("hub"),
            &hub_repo(),
            12,
            PrWindowActivity::Review(skill),
        );
        assert_eq!(window.label().as_str(), expected);
        assert_eq!(
            format!("PR 12 (alice): {}", &skill.slash_command()[1..]),
            expected
        );
    }

    /// In Rex an investigation and each review have their own tab; in tmux they share
    /// `hub:pr:12`, as they always have.
    #[test]
    fn a_prs_investigation_and_reviews_share_a_tmux_name_but_not_a_label() {
        let windows = [
            PrWindowActivity::Investigate,
            PrWindowActivity::Review(ReviewSkill::ReviewCode),
            PrWindowActivity::Review(ReviewSkill::ReviewConverge),
            PrWindowActivity::Review(ReviewSkill::ReviewPrCommentsConverge),
        ]
        .map(|activity| on_pr(&short("hub"), &hub_repo(), 12, activity));
        let labels: std::collections::HashSet<&str> = windows
            .iter()
            .map(|window| window.label().as_str())
            .collect();
        assert_eq!(labels.len(), 4);
        for window in &windows {
            assert_eq!(window.to_string(), "hub:pr:12");
        }
    }
}
