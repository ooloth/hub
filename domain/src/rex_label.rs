//! The label a Rex tab hub opens carries: `<subject> (<author login>): <activity>`.

use crate::window_segment::{segment, short_hash};
use crate::GitHubLogin;

/// Longest label hub gives a tab.
const MAX_LABEL: usize = 120;

/// Longest piece of a signal's own text, such as an alert's excerpt, a CI workflow's name or a
/// media item's title, that a label carries.
const MAX_TEXT: usize = 40;

/// Longest environment name a label carries.
const MAX_ENV: usize = 20;

/// Length of the hash that keeps apart two texts whose excerpts would otherwise read the same.
const HASH_LEN: usize = 6;

/// Stands in for text that cleans to nothing.
const EMPTY_TEXT: &str = "unnamed";

/// The label of a Rex tab hub opens, by which hub finds the tab again (decision 033).
///
/// Built only from a fixed subject word followed by cleaned text, so it is never empty, has no
/// control characters or line breaks, is at most [`MAX_LABEL`] characters and never starts with
/// `-`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RexLabel(String);

impl RexLabel {
    /// `PR <number> (<author>): <activity>`.
    pub(crate) fn pr(number: u64, author: &GitHubLogin, activity: &str) -> Self {
        Self::new(format!("PR {number} ({author}): {activity}"))
    }

    /// `Issue <number> (<author>): investigate`.
    pub(crate) fn issue(number: u64, author: &GitHubLogin) -> Self {
        Self::new(format!("Issue {number} ({author}): investigate"))
    }

    /// `CI <workflow>: investigate`.
    pub(crate) fn ci(workflow: &str) -> Self {
        Self::new(format!(
            "CI {}: investigate",
            excerpt(workflow, workflow, false)
        ))
    }

    /// `<source> <env> <excerpt>: investigate`.
    ///
    /// The excerpt carries a hash of the environment and the message whenever either is not shown
    /// whole, so two alerts never share a label, and so never share a tab.
    pub(crate) fn alert(source: &str, env: &str, message: &str) -> Self {
        let mut shown_env = segment(env);
        shown_env.truncate(MAX_ENV);
        let shown_env = shown_env.trim_end_matches('-').to_string();
        let identity = format!("{env}\u{1}{message}");
        let message = excerpt(message, &identity, shown_env != env);
        Self::new(format!("{source} {shown_env} {message}: investigate"))
    }

    /// `Media <title>: investigate`.
    pub(crate) fn media(title: &str) -> Self {
        Self::new(format!(
            "Media {}: investigate",
            excerpt(title, title, false)
        ))
    }

    fn new(label: String) -> Self {
        assert!(!label.is_empty(), "a tab label must not be empty");
        assert!(
            !label.contains(char::is_control),
            "tab label {label:?} has a control character"
        );
        assert!(
            label.chars().count() <= MAX_LABEL,
            "tab label {label:?} exceeds {MAX_LABEL} characters"
        );
        assert!(
            !label.starts_with('-'),
            "tab label {label:?} would be read as a flag"
        );
        Self(label)
    }

    /// The label as Rex shows and matches it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for RexLabel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// `raw` with every run of whitespace and control characters made one space, and trimmed.
fn clean(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        if ch.is_whitespace() || ch.is_control() {
            if !out.is_empty() && !out.ends_with(' ') {
                out.push(' ');
            }
        } else {
            out.push(ch);
        }
    }
    let trimmed = out.trim_end();
    if trimmed.is_empty() {
        EMPTY_TEXT.to_string()
    } else {
        trimmed.to_string()
    }
}

/// `raw` cleaned and cut to [`MAX_TEXT`] characters.
///
/// When the excerpt is not `raw` exactly, or `marked` says something beside it was not shown
/// whole, it ends in `…` and a hash of `identity`, so two texts that clean or cut to the same
/// excerpt still give different labels.
fn excerpt(raw: &str, identity: &str, marked: bool) -> String {
    let cleaned = clean(raw);
    if !marked && cleaned == raw && cleaned.chars().count() <= MAX_TEXT {
        return cleaned;
    }
    let keep: String = cleaned.chars().take(MAX_TEXT - HASH_LEN - 2).collect();
    format!("{}… {}", keep.trim_end(), short_hash(identity, HASH_LEN))
}

#[cfg(test)]
mod tests {
    use super::{RexLabel, MAX_LABEL};
    use crate::GitHubLogin;
    use proptest::prelude::*;
    use rstest::rstest;

    fn alice() -> GitHubLogin {
        GitHubLogin::parse("alice").unwrap()
    }

    fn assert_well_formed(label: &RexLabel) {
        let text = label.as_str();
        assert!(
            !text.contains(char::is_control),
            "{text:?} has a control character"
        );
        assert!(text.chars().count() <= MAX_LABEL, "{text:?} is too long");
    }

    #[rstest]
    #[case(RexLabel::pr(12, &alice(), "investigate"), "PR 12 (alice): investigate")]
    #[case(RexLabel::issue(412, &alice()), "Issue 412 (alice): investigate")]
    #[case(RexLabel::ci("test-suite"), "CI test-suite: investigate")]
    #[case(RexLabel::ci("CI / build (push)"), "CI CI / build (push): investigate")]
    #[case(
        RexLabel::alert("Loki", "prod", "connection refused"),
        "Loki prod connection refused: investigate"
    )]
    #[case(
        RexLabel::media("Some Show - S01E01"),
        "Media Some Show - S01E01: investigate"
    )]
    fn text_shown_whole_is_left_as_it_is(#[case] label: RexLabel, #[case] expected: &str) {
        assert_eq!(label.as_str(), expected);
    }

    /// A long or multi-line message is not shown whole, so its excerpt says so with a hash.
    #[rstest]
    #[case("connection refused by the upstream database after 30 seconds")]
    #[case("connection\nrefused")]
    #[case("  padded")]
    #[case("")]
    fn text_not_shown_whole_ends_in_a_hash(#[case] message: &str) {
        let label = RexLabel::alert("Loki", "prod", message);
        let text = label.as_str();
        let excerpt = text
            .strip_prefix("Loki prod ")
            .and_then(|rest| rest.strip_suffix(": investigate"))
            .unwrap_or_else(|| panic!("{text:?} is not an alert label"));
        let (shown, hash) = excerpt
            .rsplit_once("… ")
            .unwrap_or_else(|| panic!("{text:?} has no hash"));
        assert_eq!(hash.len(), 6, "{text:?}");
        assert!(shown.chars().count() <= 32, "{text:?}");
    }

    #[test]
    fn a_long_excerpt_keeps_the_start_of_the_message() {
        let label = RexLabel::alert(
            "Loki",
            "prod",
            "connection refused by the upstream database after 30 seconds",
        );
        assert!(
            label
                .as_str()
                .starts_with("Loki prod connection refused by the upstre… "),
            "{label}"
        );
    }

    /// Cleaning makes these two messages read the same, so only the hash keeps their tabs apart.
    #[test]
    fn two_messages_that_differ_only_in_whitespace_get_different_labels() {
        assert_ne!(
            RexLabel::alert("Loki", "prod", "connection\nrefused"),
            RexLabel::alert("Loki", "prod", "connection refused"),
        );
    }

    #[test]
    fn an_environment_not_shown_whole_marks_the_excerpt() {
        let label = RexLabel::alert("Loki", "Prod EU", "boom");
        assert!(label.as_str().starts_with("Loki prod-eu boom… "), "{label}");
    }

    proptest! {
        /// Untrusted text of any shape gives a well-formed label. Each case also runs the
        /// assertions in `RexLabel::new`.
        #[test]
        fn any_alert_gives_a_well_formed_label(env in ".*", message in ".*") {
            let label = RexLabel::alert("Loki", &env, &message);
            assert_well_formed(&label);
            prop_assert!(label.as_str().starts_with("Loki "), "{}", label);
        }

        #[test]
        fn any_workflow_or_title_gives_a_well_formed_label(text in ".*") {
            assert_well_formed(&RexLabel::ci(&text));
            assert_well_formed(&RexLabel::media(&text));
        }

        /// Two alerts must never share a tab. This holds up to a collision of the six-character
        /// hash. The generators lean towards whitespace and shared prefixes, where cleaning and
        /// cutting would otherwise make two messages read the same.
        #[test]
        fn two_different_alerts_never_share_a_label(
            env_a in "(prod|Prod| prod|prod-eu|[a-z ]{0,25})",
            env_b in "(prod|Prod| prod|prod-eu|[a-z ]{0,25})",
            message_a in "(connection refused by the upstream database )?[a-z \\n\\t]{0,20}",
            message_b in "(connection refused by the upstream database )?[a-z \\n\\t]{0,20}",
        ) {
            prop_assume!((&env_a, &message_a) != (&env_b, &message_b));
            prop_assert_ne!(
                RexLabel::alert("Loki", &env_a, &message_a),
                RexLabel::alert("Loki", &env_b, &message_b),
            );
        }

        #[test]
        fn two_different_workflows_never_share_a_label(
            a in "(build and test everything on every platform )?[a-z \\n]{0,20}",
            b in "(build and test everything on every platform )?[a-z \\n]{0,20}",
        ) {
            prop_assume!(a != b);
            prop_assert_ne!(RexLabel::ci(&a), RexLabel::ci(&b));
        }
    }
}
