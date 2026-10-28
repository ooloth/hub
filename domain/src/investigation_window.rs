//! Deterministic names for the windows hub opens, in tmux or Rex.

use uuid::Uuid;

use crate::ProjectShortName;

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

/// Length of the hash that tells two shortened segments apart. It separates a
/// small population — segments that share a prefix and are both over budget —
/// so four characters buys back legibility without risking a clash.
const TRUNCATION_HASH_LEN: usize = 4;

/// Length of the hash that *is* an alert's identity. Every distinct alert
/// depends on this one, so it gets a wider space than a truncation tie-break.
const IDENTITY_HASH_LEN: usize = 6;

/// Substituted for a segment whose input sanitises to nothing, so that
/// legitimately punctuation-only external data cannot produce an empty name.
const EMPTY_SEGMENT: &str = "unnamed";

/// Namespace for the v5 UUIDs the short hashes are taken from. Any fixed UUID
/// works; this one is arbitrary and must never change, because changing it
/// renames every alert window and breaks the match in #331.
const HASH_NAMESPACE: Uuid = Uuid::from_bytes([
    0x1f, 0x9c, 0x4a, 0x2e, 0x7b, 0x60, 0x5d, 0x48, 0x9e, 0x11, 0xc3, 0x07, 0x5a, 0x8f, 0x2d, 0x64,
]);

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
}

/// The name of a window hub opened, in tmux or Rex, addressable afterwards.
///
/// Every name is `<project>:<kind>:<discriminator>`, built through one
/// constructor per signal kind. The project segment is always a
/// [`ProjectShortName`], so a configured `short_name` cannot be bypassed.
///
/// The kind segment comes from the constructor rather than the caller. It is
/// what keeps the windows one pull request can have apart: its investigation,
/// lazygit, Octo and diff windows share a project and a number and differ
/// only in kind.
///
/// Construction is deterministic in its inputs. That is the property that lets
/// a later launch find the window an earlier one opened, by building the same
/// name again rather than by recording it anywhere.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvestigationWindow(String);

impl InvestigationWindow {
    /// Window for an investigation of a pull request.
    ///
    /// # Panics
    /// Panics if `number` is zero.
    #[must_use]
    pub fn pr(project: &ProjectShortName, number: u64) -> Self {
        Self::numbered(project, "pr", number)
    }

    /// Window for an investigation of a GitHub issue.
    ///
    /// # Panics
    /// Panics if `number` is zero.
    #[must_use]
    pub fn issue(project: &ProjectShortName, number: u64) -> Self {
        Self::numbered(project, "issue", number)
    }

    /// Window for an investigation of a failing CI workflow.
    ///
    /// Named by workflow rather than by run, so re-running a failing workflow
    /// maps to the window already investigating it.
    #[must_use]
    pub fn ci(project: &ProjectShortName, workflow: &str) -> Self {
        Self::for_project(project, "ci", &segment(workflow))
    }

    /// Window for an investigation of a log alert.
    ///
    /// `message` is the stable error category, not the human-readable query
    /// title, which several distinct alerts share. Scoped by `env`, since the
    /// same error in two environments is two investigations.
    #[must_use]
    pub fn alert(
        project: &ProjectShortName,
        source: AlertSource,
        env: &str,
        message: &str,
    ) -> Self {
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
        Self::for_project(project, source.kind(), &discriminator)
    }

    /// Window for an investigation of a blocked media item.
    ///
    /// These signals carry no repository or number, so the item's title is the
    /// only thing distinguishing one from another.
    #[must_use]
    pub fn media(title: &str) -> Self {
        Self::build("media", "item", &segment(title))
    }

    /// Window for a pull request opened in lazygit.
    ///
    /// # Panics
    /// Panics if `number` is zero.
    #[must_use]
    pub fn lazygit(project: &ProjectShortName, number: u64) -> Self {
        Self::numbered(project, "git", number)
    }

    /// Window for a pull request opened in neovim's Octo.
    ///
    /// # Panics
    /// Panics if `number` is zero.
    #[must_use]
    pub fn octo(project: &ProjectShortName, number: u64) -> Self {
        Self::numbered(project, "octo", number)
    }

    /// Window for a pull request's diff.
    ///
    /// # Panics
    /// Panics if `number` is zero.
    #[must_use]
    pub fn diff(project: &ProjectShortName, number: u64) -> Self {
        Self::numbered(project, "diff", number)
    }

    fn numbered(project: &ProjectShortName, kind: &str, number: u64) -> Self {
        assert!(number > 0, "signal number must be positive, got {number}");
        Self::for_project(project, kind, &number.to_string())
    }

    /// A window named for `project`, whose short name already fits the
    /// project segment and so must arrive in it unchanged.
    fn for_project(project: &ProjectShortName, kind: &str, discriminator: &str) -> Self {
        let window = Self::build(project.as_str(), kind, discriminator);
        assert_eq!(
            window.0.split(':').next(),
            Some(project.as_str()),
            "window {window} does not start with the short name {project}"
        );
        window
    }

    fn build(project: &str, kind: &str, discriminator: &str) -> Self {
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

        Self(name)
    }
}

impl std::fmt::Display for InvestigationWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Reduces external text to `[a-z0-9-]`, collapsing runs of separators.
///
/// Workflow names, project names and environment names come from GitHub, Loki
/// and `hub.toml`, where spaces, slashes and colons are all legitimate. They
/// are rewritten rather than rejected, and text that reduces to nothing gets a
/// placeholder, so no valid input can produce an empty name.
pub(crate) fn segment(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            out.extend(ch.to_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }

    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        EMPTY_SEGMENT.to_string()
    } else {
        trimmed.to_string()
    }
}

/// Shortens a sanitised segment to `max`, ending it with a hash of the full
/// segment so that two long inputs sharing a prefix stay distinguishable.
///
/// Segments are ASCII by the time they reach here, so slicing by byte index
/// cannot split a character.
pub(crate) fn clamp(seg: &str, max: usize) -> String {
    assert!(
        max > TRUNCATION_HASH_LEN + 1,
        "segment budget {max} leaves no room for a hash"
    );
    if seg.len() <= max {
        return seg.to_string();
    }
    let keep = max - TRUNCATION_HASH_LEN - 1;
    // Trimmed so the kept text cannot end on the separator the hash adds.
    let prefix = seg[..keep].trim_end_matches('-');
    format!("{prefix}-{}", short_hash(seg, TRUNCATION_HASH_LEN))
}

/// Short, stable hash of arbitrary text, for disambiguating names that would
/// otherwise collide.
fn short_hash(raw: &str, len: usize) -> String {
    Uuid::new_v5(&HASH_NAMESPACE, raw.as_bytes())
        .simple()
        .to_string()
        .chars()
        .take(len)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{AlertSource, InvestigationWindow, MAX_LEN};
    use crate::ProjectShortName;
    use proptest::prelude::*;
    use rstest::rstest;

    /// The short name a project tracking `repo` gets when it chooses none.
    fn of(repo: &str) -> ProjectShortName {
        ProjectShortName::from_repo(&repo.parse().unwrap())
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
            assert_well_formed(&InvestigationWindow::pr(&project, number).to_string());
        }

        /// A short name is chosen to be what the window shows, so the window
        /// must not rewrite or shorten it.
        #[test]
        fn the_project_segment_is_the_short_name_unchanged(
            project in short_names(),
            number in 1u64..=u64::MAX,
        ) {
            let name = InvestigationWindow::pr(&project, number).to_string();
            prop_assert_eq!(name.split(':').next(), Some(project.as_str()));
        }

        /// Workflow names are the least constrained input: GitHub allows
        /// punctuation, emoji and considerable length.
        #[test]
        fn ci_window_is_well_formed_for_any_workflow(project in short_names(), workflow in ".*") {
            assert_well_formed(&InvestigationWindow::ci(&project, &workflow).to_string());
        }

        #[test]
        fn alert_window_is_well_formed_for_any_message(
            project in short_names(),
            env in ".*",
            message in ".*",
        ) {
            let window = InvestigationWindow::alert(&project, AlertSource::Loki, &env, &message);
            assert_well_formed(&window.to_string());
        }

        /// #331 finds an open window by building its name again, so equal
        /// inputs must always give equal names.
        #[test]
        fn construction_is_deterministic(project in short_names(), workflow in ".*") {
            prop_assert_eq!(
                InvestigationWindow::ci(&project, &workflow),
                InvestigationWindow::ci(&project, &workflow),
            );
        }

        /// Two alerts must be investigable side by side. Distinctness rests on
        /// a truncated hash, so this holds up to a hash collision.
        #[test]
        fn distinct_alert_messages_get_distinct_windows(a in ".*", b in ".*") {
            prop_assume!(a != b);
            prop_assert_ne!(
                InvestigationWindow::alert(&short("api"), AlertSource::Loki, "prod", &a),
                InvestigationWindow::alert(&short("api"), AlertSource::Loki, "prod", &b),
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
            let name = InvestigationWindow::ci(&project, &workflow).to_string();
            prop_assert!(!name.contains("--"), "{} has a doubled separator", name);
        }

        /// The same error in two environments is two investigations.
        #[test]
        fn distinct_alert_environments_get_distinct_windows(a in ".*", b in ".*") {
            prop_assume!(a != b);
            prop_assert_ne!(
                InvestigationWindow::alert(&short("api"), AlertSource::Loki, &a, "boom"),
                InvestigationWindow::alert(&short("api"), AlertSource::Loki, &b, "boom"),
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
                InvestigationWindow::pr(&project, number),
                InvestigationWindow::issue(&project, number),
                InvestigationWindow::lazygit(&project, number),
                InvestigationWindow::octo(&project, number),
                InvestigationWindow::diff(&project, number),
            ]
            .iter()
            .map(ToString::to_string)
            .collect();
            prop_assert_eq!(names.len(), 5);
        }
    }

    #[rstest]
    #[case(InvestigationWindow::pr(&of("ooloth/hub"), 330), "hub:pr:330")]
    #[case(InvestigationWindow::issue(&of("ooloth/hub"), 412), "hub:issue:412")]
    #[case(InvestigationWindow::lazygit(&of("ooloth/hub"), 330), "hub:git:330")]
    #[case(InvestigationWindow::octo(&of("ooloth/hub"), 330), "hub:octo:330")]
    #[case(InvestigationWindow::diff(&of("ooloth/hub"), 330), "hub:diff:330")]
    fn the_owner_is_dropped_and_the_kind_names_the_window(
        #[case] window: InvestigationWindow,
        #[case] expected: &str,
    ) {
        assert_eq!(window.to_string(), expected);
    }

    /// A chosen short name is the whole point: it replaces the repository's.
    #[test]
    fn a_chosen_short_name_names_the_window() {
        assert_eq!(
            InvestigationWindow::pr(&short("h"), 7).to_string(),
            "h:pr:7"
        );
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
            InvestigationWindow::ci(&short("hub"), workflow).to_string(),
            expected
        );
    }

    /// External text that sanitises to nothing must not produce an empty name.
    #[test]
    fn a_workflow_named_only_with_punctuation_falls_back() {
        let name = InvestigationWindow::ci(&short("hub"), "!!! ///").to_string();
        assert_eq!(name, "hub:ci:unnamed");
    }

    /// A long project must not truncate the kind away, or the window stops
    /// saying what it is.
    #[test]
    fn a_long_project_keeps_its_kind_and_discriminator() {
        let name =
            InvestigationWindow::pr(&of("ooloth/a-very-long-repository-name"), 330).to_string();
        assert!(name.ends_with(":pr:330"), "{name} lost its kind or number");
        assert!(name.len() <= MAX_LEN, "{name} is too long");
    }

    /// A shortened repository still has to be recognisable at a glance, so the
    /// disambiguating hash is kept short enough to leave readable characters.
    #[test]
    fn a_shortened_repository_stays_recognisable() {
        let name =
            InvestigationWindow::pr(&of("ooloth/a-very-long-repository-name"), 330).to_string();
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
        assert_eq!(InvestigationWindow::pr(&of(repo), 12).to_string(), expected);
    }

    /// The case that surfaced this: the kept prefix ends on a separator, and
    /// the hash separator would double it.
    #[test]
    fn a_prefix_ending_in_a_separator_does_not_double_it() {
        let name =
            InvestigationWindow::ci(&short("hub"), "build and test everything nightly").to_string();
        assert!(!name.contains("--"), "{name} has a doubled separator");
    }

    /// Truncation must not be able to manufacture a collision.
    #[test]
    fn two_long_projects_sharing_a_prefix_get_different_windows() {
        assert_ne!(
            InvestigationWindow::pr(&of("ooloth/a-very-long-repository-name-one"), 1),
            InvestigationWindow::pr(&of("ooloth/a-very-long-repository-name-two"), 1),
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

    #[test]
    #[should_panic(expected = "signal number must be positive")]
    fn a_zero_signal_number_is_a_programmer_error() {
        let _ = InvestigationWindow::pr(&of("ooloth/hub"), 0);
    }
}
