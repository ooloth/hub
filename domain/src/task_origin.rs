use serde::{Deserialize, Serialize};

use crate::pr::RepoSlug;

/// The originating signal a task was created from — its provenance.
///
/// Recorded separately from the freeform `links` field and from the editable
/// `repo` dispatch target, so hub can reliably correlate a task back to the
/// PR / issue / CI run / alert it came from. This typed handle is what
/// fold-back, badge/dedup, and the feedback-loop delta key on.
///
/// Serialized as an internally tagged JSON object (`{"type":"pr",…}`) so the
/// wire shape is self-describing and a new variant never silently deserializes
/// as another. Every variant carries a *stable identity* — the fields that are
/// invariant for "the same underlying signal" re-observed on a later scan.
///
/// This type is **independent of the `private` feature**: every variant —
/// including `Alert { source: Media }` — is always present, so a build without
/// `private` (e.g. a work laptop with no media signals) deserializes and
/// renders any persisted origin without error. Only the *construction* of a
/// media-sourced alert, at signal-seed time, is `private`-gated.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum TaskOrigin {
    /// A pull request. Identity = `(repo, number)`.
    Pr { repo: RepoSlug, number: u64 },
    /// A ticket in any tracker, tagged by `system`. `repo` is `Some` for
    /// GitHub and `None` for Linear, whose `id` (e.g. `ENG-123`) is globally
    /// unique. Identity = `(system, id)`.
    Issue {
        system: IssueSystem,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        repo: Option<RepoSlug>,
        id: String,
    },
    /// A CI failure. Identity = `(repo, workflow, job, step)`; `url` is the
    /// per-run display/navigation handle and is deliberately excluded from
    /// identity (it changes on every run). When `job`/`step` are both `None`,
    /// identity collapses to `(repo, workflow)` — "this workflow is failing".
    Ci {
        repo: RepoSlug,
        workflow: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        job: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        step: Option<String>,
        url: String,
    },
    /// A problem surfaced by scanning a running system — log errors, GCP log
    /// entries, media-server health — none of which has an external tracker.
    /// `key` is the stable correlation handle (`(source, key)` is the
    /// identity); `label` is the display string only and may change freely
    /// between scans.
    Alert {
        source: AlertSource,
        key: String,
        label: String,
    },
    /// A blank task with no originating signal.
    #[default]
    Idea,
}

/// The ticket system an [`TaskOrigin::Issue`] came from. Extensible to
/// Jira/Monday/Trekker; only GitHub and Linear are populated today.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IssueSystem {
    GitHub,
    Linear,
}

/// The scan source an [`TaskOrigin::Alert`] came from. `Media` is always
/// present regardless of the `private` feature (see [`TaskOrigin`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AlertSource {
    Loki,
    Gcp,
    Media,
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn slug() -> RepoSlug {
        RepoSlug::new("ooloth", "hub")
    }

    // ── wire shape (the cross-build serialization contract) ────────────────────

    #[rstest::rstest]
    #[case(
        TaskOrigin::Pr { repo: slug(), number: 42 },
        r#"{"type":"pr","repo":"ooloth/hub","number":42}"#
    )]
    #[case(
        TaskOrigin::Issue { system: IssueSystem::GitHub, repo: Some(slug()), id: "7".into() },
        r#"{"type":"issue","system":"github","repo":"ooloth/hub","id":"7"}"#
    )]
    #[case(
        TaskOrigin::Issue { system: IssueSystem::Linear, repo: None, id: "ENG-1".into() },
        r#"{"type":"issue","system":"linear","id":"ENG-1"}"#
    )]
    #[case(
        TaskOrigin::Ci { repo: slug(), workflow: "ci".into(), job: Some("test".into()), step: Some("unit".into()), url: "u".into() },
        r#"{"type":"ci","repo":"ooloth/hub","workflow":"ci","job":"test","step":"unit","url":"u"}"#
    )]
    #[case(
        TaskOrigin::Ci { repo: slug(), workflow: "ci".into(), job: None, step: None, url: "u".into() },
        r#"{"type":"ci","repo":"ooloth/hub","workflow":"ci","url":"u"}"#
    )]
    #[case(
        TaskOrigin::Alert { source: AlertSource::Loki, key: "k".into(), label: "l".into() },
        r#"{"type":"alert","source":"loki","key":"k","label":"l"}"#
    )]
    #[case(
        TaskOrigin::Alert { source: AlertSource::Gcp, key: "k".into(), label: "l".into() },
        r#"{"type":"alert","source":"gcp","key":"k","label":"l"}"#
    )]
    #[case(
        TaskOrigin::Alert { source: AlertSource::Media, key: "k".into(), label: "l".into() },
        r#"{"type":"alert","source":"media","key":"k","label":"l"}"#
    )]
    #[case(TaskOrigin::Idea, r#"{"type":"idea"}"#)]
    fn serializes_to_tagged_wire_shape(#[case] origin: TaskOrigin, #[case] expected: &str) {
        assert_eq!(serde_json::to_string(&origin).unwrap(), expected);
    }

    // ── default ────────────────────────────────────────────────────────────────

    #[test]
    fn default_is_idea() {
        assert_eq!(TaskOrigin::default(), TaskOrigin::Idea);
    }

    #[test]
    fn idea_deserializes_from_tagged_object() {
        let origin: TaskOrigin = serde_json::from_str(r#"{"type":"idea"}"#).unwrap();
        assert_eq!(origin, TaskOrigin::Idea);
    }

    // ── private-feature independence ─────────────────────────────────────────────

    #[test]
    fn media_alert_deserializes_without_private_feature() {
        // `domain` is compiled without the `private` feature, so this test runs
        // in a non-`private` build. It must still read a media-sourced origin
        // persisted by a `private` build — the cross-build hazard this design
        // exists to avoid.
        let origin: TaskOrigin = serde_json::from_str(
            r#"{"type":"alert","source":"media","key":"media/blocked/sonarr/x","label":"x"}"#,
        )
        .unwrap();
        assert_eq!(
            origin,
            TaskOrigin::Alert {
                source: AlertSource::Media,
                key: "media/blocked/sonarr/x".into(),
                label: "x".into(),
            }
        );
    }

    // ── round trip across the whole variant space ────────────────────────────────

    fn arb_origin() -> impl Strategy<Value = TaskOrigin> {
        let text = "[a-zA-Z0-9/ ._:-]{0,40}";
        prop_oneof![
            any::<u64>().prop_map(|number| TaskOrigin::Pr {
                repo: slug(),
                number
            }),
            (
                prop_oneof![Just(IssueSystem::GitHub), Just(IssueSystem::Linear)],
                prop::option::of(Just(slug())),
                text,
            )
                .prop_map(|(system, repo, id)| TaskOrigin::Issue { system, repo, id }),
            (text, prop::option::of(text), prop::option::of(text), text).prop_map(
                |(workflow, job, step, url)| TaskOrigin::Ci {
                    repo: slug(),
                    workflow,
                    job,
                    step,
                    url,
                }
            ),
            (
                prop_oneof![
                    Just(AlertSource::Loki),
                    Just(AlertSource::Gcp),
                    Just(AlertSource::Media)
                ],
                text,
                text,
            )
                .prop_map(|(source, key, label)| TaskOrigin::Alert {
                    source,
                    key,
                    label
                }),
            Just(TaskOrigin::Idea),
        ]
    }

    proptest! {
        #[test]
        fn round_trips_through_json(origin in arb_origin()) {
            let json = serde_json::to_string(&origin).unwrap();
            let back: TaskOrigin = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(origin, back);
        }
    }
}
