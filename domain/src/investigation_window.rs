//! Deterministic tmux window names for the windows hub opens.

use uuid::Uuid;

/// Longest window name hub will produce. Beyond this the tmux status bar
/// crowds out everything else.
const MAX_LEN: usize = 32;

/// Budgets for the three segments. They sum, with the two separators, to
/// exactly `MAX_LEN`, which is what makes the length postcondition in `build`
/// a check on this code rather than a trap for long external names.
const MAX_PROJECT: usize = 12;
const MAX_KIND: usize = 5;
const MAX_DISCRIMINATOR: usize = 13;

/// Length of the disambiguating hash appended to a truncated name or derived
/// from an alert's error category.
const HASH_LEN: usize = 6;

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

/// The name of a tmux window hub opened, addressable afterwards.
///
/// Every name is `<project>:<kind>:<discriminator>`, built through one
/// constructor per signal kind. The kind segment comes from the constructor
/// rather than the caller, so a pull request and an issue sharing a number
/// cannot produce the same name.
///
/// Construction is deterministic in its inputs. That is the property that lets
/// a later launch find the window an earlier one opened, by building the same
/// name again rather than by recording it anywhere.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvestigationWindow(String);

impl InvestigationWindow {
    /// Window for an investigation of a pull request.
    ///
    /// `repo` may be an `owner/name` slug; only the name is used.
    ///
    /// # Panics
    /// Panics if `number` is zero.
    #[must_use]
    pub fn pr(repo: &str, number: u64) -> Self {
        Self::numbered(repo, "pr", number)
    }

    /// Window for an investigation of a GitHub issue.
    ///
    /// # Panics
    /// Panics if `number` is zero.
    #[must_use]
    pub fn issue(repo: &str, number: u64) -> Self {
        Self::numbered(repo, "issue", number)
    }

    /// Window for an investigation of a failing CI workflow.
    ///
    /// Named by workflow rather than by run, so re-running a failing workflow
    /// maps to the window already investigating it.
    #[must_use]
    pub fn ci(repo: &str, workflow: &str) -> Self {
        Self::build(repo_name(repo), "ci", &segment(workflow))
    }

    /// Window for an investigation of a log alert.
    ///
    /// `message` is the stable error category, not the human-readable query
    /// title, which several distinct alerts share. Scoped by `env`, since the
    /// same error in two environments is two investigations.
    #[must_use]
    pub fn alert(project: &str, source: AlertSource, env: &str, message: &str) -> Self {
        // Uniqueness rests entirely on the hash, which covers the environment
        // as well as the message. The readable environment prefix is
        // decoration, so shortening it cannot make two alerts collide.
        let mut readable = segment(env);
        readable.truncate(MAX_DISCRIMINATOR - HASH_LEN - 1);
        let discriminator = format!("{readable}-{}", short_hash(&format!("{env}\u{1}{message}")));
        Self::build(project, source.kind(), &discriminator)
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
    pub fn lazygit(repo: &str, number: u64) -> Self {
        Self::numbered(repo, "git", number)
    }

    /// Window for a pull request opened in neovim's Octo.
    ///
    /// # Panics
    /// Panics if `number` is zero.
    #[must_use]
    pub fn octo(repo: &str, number: u64) -> Self {
        Self::numbered(repo, "octo", number)
    }

    /// Window for a pull request's diff.
    ///
    /// # Panics
    /// Panics if `number` is zero.
    #[must_use]
    pub fn diff(repo: &str, number: u64) -> Self {
        Self::numbered(repo, "diff", number)
    }

    fn numbered(repo: &str, kind: &str, number: u64) -> Self {
        assert!(number > 0, "signal number must be positive, got {number}");
        Self::build(repo_name(repo), kind, &number.to_string())
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
fn segment(raw: &str) -> String {
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

/// The repository half of an `owner/name` slug. Applied only to slugs, never
/// to workflow names, where a slash is ordinary punctuation.
fn repo_name(repo: &str) -> &str {
    repo.rsplit('/').next().unwrap_or(repo)
}

/// Shortens a sanitised segment to `max`, ending it with a hash of the full
/// segment so that two long inputs sharing a prefix stay distinguishable.
///
/// Segments are ASCII by the time they reach here, so slicing by byte index
/// cannot split a character.
fn clamp(seg: &str, max: usize) -> String {
    if seg.len() <= max {
        return seg.to_string();
    }
    let keep = max - HASH_LEN - 1;
    format!("{}-{}", &seg[..keep], short_hash(seg))
}

/// Short, stable hash of arbitrary text, for disambiguating names that would
/// otherwise collide.
fn short_hash(raw: &str) -> String {
    Uuid::new_v5(&HASH_NAMESPACE, raw.as_bytes())
        .simple()
        .to_string()
        .chars()
        .take(HASH_LEN)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{AlertSource, InvestigationWindow, MAX_LEN};
    use proptest::prelude::*;
    use rstest::rstest;

    fn assert_well_formed(name: &str) {
        assert!(!name.is_empty(), "empty name");
        assert!(name.len() <= MAX_LEN, "{name} is longer than {MAX_LEN}");
        assert!(!name.contains(char::is_whitespace), "{name} has whitespace");
    }

    proptest! {
        /// The three postconditions hold for repositories the author did not
        /// imagine. Each case also exercises the assertions inside `build`.
        #[test]
        fn pr_window_is_well_formed_for_any_repo(repo in ".*", number in 1u64..=u64::MAX) {
            assert_well_formed(&InvestigationWindow::pr(&repo, number).to_string());
        }

        /// Workflow names are the least constrained input: GitHub allows
        /// punctuation, emoji and considerable length.
        #[test]
        fn ci_window_is_well_formed_for_any_workflow(repo in ".*", workflow in ".*") {
            assert_well_formed(&InvestigationWindow::ci(&repo, &workflow).to_string());
        }

        #[test]
        fn alert_window_is_well_formed_for_any_message(
            project in ".*",
            env in ".*",
            message in ".*",
        ) {
            let window = InvestigationWindow::alert(&project, AlertSource::Loki, &env, &message);
            assert_well_formed(&window.to_string());
        }

        /// #331 finds an open window by building its name again, so equal
        /// inputs must always give equal names.
        #[test]
        fn construction_is_deterministic(repo in ".*", workflow in ".*") {
            prop_assert_eq!(
                InvestigationWindow::ci(&repo, &workflow),
                InvestigationWindow::ci(&repo, &workflow),
            );
        }

        /// Two alerts must be investigable side by side. Distinctness rests on
        /// a truncated hash, so this holds up to a hash collision.
        #[test]
        fn distinct_alert_messages_get_distinct_windows(a in ".*", b in ".*") {
            prop_assume!(a != b);
            prop_assert_ne!(
                InvestigationWindow::alert("api", AlertSource::Loki, "prod", &a),
                InvestigationWindow::alert("api", AlertSource::Loki, "prod", &b),
            );
        }

        /// The same error in two environments is two investigations.
        #[test]
        fn distinct_alert_environments_get_distinct_windows(a in ".*", b in ".*") {
            prop_assume!(a != b);
            prop_assert_ne!(
                InvestigationWindow::alert("api", AlertSource::Loki, &a, "boom"),
                InvestigationWindow::alert("api", AlertSource::Loki, &b, "boom"),
            );
        }
    }

    /// The collision that motivated putting the kind in the name: a repository
    /// numbers its pull requests and issues from one sequence.
    #[test]
    fn a_pull_request_and_an_issue_sharing_a_number_get_different_windows() {
        assert_ne!(
            InvestigationWindow::pr("ooloth/hub", 330),
            InvestigationWindow::issue("ooloth/hub", 330),
        );
    }

    #[rstest]
    #[case(InvestigationWindow::pr("ooloth/hub", 330), "hub:pr:330")]
    #[case(InvestigationWindow::issue("ooloth/hub", 412), "hub:issue:412")]
    #[case(InvestigationWindow::lazygit("ooloth/hub", 330), "hub:git:330")]
    #[case(InvestigationWindow::octo("ooloth/hub", 330), "hub:octo:330")]
    #[case(InvestigationWindow::diff("ooloth/hub", 330), "hub:diff:330")]
    fn the_owner_is_dropped_and_the_kind_names_the_window(
        #[case] window: InvestigationWindow,
        #[case] expected: &str,
    ) {
        assert_eq!(window.to_string(), expected);
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
            InvestigationWindow::ci("hub", workflow).to_string(),
            expected
        );
    }

    /// External text that sanitises to nothing must not produce an empty name.
    #[test]
    fn a_workflow_named_only_with_punctuation_falls_back() {
        let name = InvestigationWindow::ci("hub", "!!! ///").to_string();
        assert_eq!(name, "hub:ci:unnamed");
    }

    /// A long project must not truncate the kind away, or the window stops
    /// saying what it is.
    #[test]
    fn a_long_project_keeps_its_kind_and_discriminator() {
        let name = InvestigationWindow::pr("ooloth/a-very-long-repository-name", 330).to_string();
        assert!(name.ends_with(":pr:330"), "{name} lost its kind or number");
        assert!(name.len() <= MAX_LEN, "{name} is too long");
    }

    /// Truncation must not be able to manufacture a collision.
    #[test]
    fn two_long_projects_sharing_a_prefix_get_different_windows() {
        assert_ne!(
            InvestigationWindow::pr("ooloth/a-very-long-repository-name-one", 1),
            InvestigationWindow::pr("ooloth/a-very-long-repository-name-two", 1),
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
        let _ = InvestigationWindow::pr("ooloth/hub", 0);
    }
}
