use serde::{Deserialize, Serialize};

use crate::ci::CiFailure;
use crate::gcp::GcpEntry;
use crate::issue::{Issue, LinearIssue};
use crate::loki::LokiEntry;
use crate::pr::{PullRequest, RepoSlug};

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

impl TaskOrigin {
    /// A pull request row. Identity is the repo and PR number.
    pub fn from_pr(pr: &PullRequest) -> Self {
        Self::Pr {
            repo: pr.repo.clone(),
            number: pr.number,
        }
    }

    /// A GitHub issue row. `id` is the issue number as a string; `repo` is
    /// always present for GitHub.
    pub fn from_issue(issue: &Issue) -> Self {
        Self::Issue {
            system: IssueSystem::GitHub,
            repo: Some(issue.repo.clone()),
            id: issue.number.to_string(),
        }
    }

    /// A Linear issue row. `id` is the globally unique identifier (e.g.
    /// `ENG-123`); Linear has no repo.
    pub fn from_linear(issue: &LinearIssue) -> Self {
        Self::Issue {
            system: IssueSystem::Linear,
            repo: None,
            id: issue.identifier.clone(),
        }
    }

    /// A CI failure row. Identity is `(repo, workflow, job, step)` — all
    /// invariant for the same failing check; `url` is kept for navigation only.
    pub fn from_ci(ci: &CiFailure) -> Self {
        Self::Ci {
            repo: ci.repo.clone(),
            workflow: ci.workflow_name.clone(),
            job: ci.job_name.clone(),
            step: ci.step_name.clone(),
            url: ci.url.clone(),
        }
    }

    /// A Loki alert row. `key` is `project/env/message` — `message` is the
    /// stable error category Loki groups by; it is the contract caller's
    /// responsibility that the query resolves `message` to a real stream label
    /// rather than the raw-line fallback. `label` is display only.
    pub fn from_loki(entry: &LokiEntry) -> Self {
        Self::Alert {
            source: AlertSource::Loki,
            key: format!("{}/{}/{}", entry.project, entry.env, entry.message),
            label: format!("{}:{} — {}", entry.project, entry.env, entry.title),
        }
    }

    /// A GCP alert row. `key` is `project/env/message` using the hub project
    /// name (not the cloud project id), symmetric with Loki. `label` is display
    /// only.
    pub fn from_gcp(entry: &GcpEntry) -> Self {
        Self::Alert {
            source: AlertSource::Gcp,
            key: format!("{}/{}/{}", entry.project, entry.env, entry.message),
            label: format!("{}:{} — {}", entry.project, entry.env, entry.title),
        }
    }
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
            r#"{"type":"alert","source":"media","key":"media/blocked/tv/x","label":"x"}"#,
        )
        .unwrap();
        assert_eq!(
            origin,
            TaskOrigin::Alert {
                source: AlertSource::Media,
                key: "media/blocked/tv/x".into(),
                label: "x".into(),
            }
        );
    }

    // ── constructors ─────────────────────────────────────────────────────────────

    fn issue() -> Issue {
        Issue {
            number: 7,
            title: "Button misaligned".into(),
            repo: slug(),
            url: "https://github.com/ooloth/hub/issues/7".into(),
            author: "bob".into(),
            age: chrono::Duration::zero(),
            urgency: crate::urgency::Urgency::Low,
            labels: vec![],
            body: None,
        }
    }

    fn linear() -> LinearIssue {
        LinearIssue {
            identifier: "ENG-1".into(),
            title: "Investigate".into(),
            url: "https://linear.app/x".into(),
            state: "Todo".into(),
            age: chrono::Duration::zero(),
            urgency: crate::urgency::Urgency::Low,
        }
    }

    fn ci(job: Option<&str>, step: Option<&str>) -> CiFailure {
        CiFailure {
            repo: slug(),
            workflow_name: "CI".into(),
            job_name: job.map(str::to_string),
            step_name: step.map(str::to_string),
            error: Some("boom".into()),
            age: chrono::Duration::zero(),
            urgency: crate::urgency::Urgency::High,
            url: "https://github.com/ooloth/hub/actions/runs/9".into(),
        }
    }

    fn loki(project: &str, env: &str, message: &str) -> LokiEntry {
        LokiEntry {
            title: "High error rate".into(),
            project: project.into(),
            env: env.into(),
            message: message.into(),
            line: r#"{"level":"error"}"#.into(),
            lookback: "15m".into(),
            age: chrono::Duration::zero(),
            urgency: crate::urgency::Urgency::Critical,
            url: "https://grafana/d/1".into(),
        }
    }

    fn gcp(project: &str, env: &str, message: &str) -> GcpEntry {
        GcpEntry {
            title: "High error rate".into(),
            project: project.into(),
            env: env.into(),
            message: message.into(),
            line: r#"{"level":"error"}"#.into(),
            lookback: "15m".into(),
            age: chrono::Duration::zero(),
            urgency: crate::urgency::Urgency::Critical,
            url: "https://console.cloud.google.com/x".into(),
            gcp_project: "rp006-prod-49a893d8".into(),
        }
    }

    #[test]
    fn from_issue_tags_github_with_repo_and_numeric_id() {
        assert_eq!(
            TaskOrigin::from_issue(&issue()),
            TaskOrigin::Issue {
                system: IssueSystem::GitHub,
                repo: Some(slug()),
                id: "7".into(),
            }
        );
    }

    #[test]
    fn from_linear_tags_linear_with_no_repo_and_identifier_id() {
        assert_eq!(
            TaskOrigin::from_linear(&linear()),
            TaskOrigin::Issue {
                system: IssueSystem::Linear,
                repo: None,
                id: "ENG-1".into(),
            }
        );
    }

    #[test]
    fn from_ci_carries_identity_fields_and_keeps_url_separate() {
        assert_eq!(
            TaskOrigin::from_ci(&ci(Some("build"), Some("test"))),
            TaskOrigin::Ci {
                repo: slug(),
                workflow: "CI".into(),
                job: Some("build".into()),
                step: Some("test".into()),
                url: "https://github.com/ooloth/hub/actions/runs/9".into(),
            }
        );
    }

    #[test]
    fn from_ci_collapses_to_workflow_when_job_and_step_absent() {
        let TaskOrigin::Ci {
            workflow,
            job,
            step,
            ..
        } = TaskOrigin::from_ci(&ci(None, None))
        else {
            panic!("expected Ci");
        };
        assert_eq!(workflow, "CI");
        assert!(job.is_none() && step.is_none());
    }

    #[test]
    fn from_loki_derives_key_and_label() {
        assert_eq!(
            TaskOrigin::from_loki(&loki("myapp", "prod", "connection refused")),
            TaskOrigin::Alert {
                source: AlertSource::Loki,
                key: "myapp/prod/connection refused".into(),
                label: "myapp:prod — High error rate".into(),
            }
        );
    }

    #[test]
    fn from_gcp_keys_on_hub_project_not_cloud_project() {
        let TaskOrigin::Alert { source, key, label } =
            TaskOrigin::from_gcp(&gcp("myapp", "prod", "oom killed"))
        else {
            panic!("expected Alert");
        };
        assert_eq!(source, AlertSource::Gcp);
        assert_eq!(key, "myapp/prod/oom killed");
        assert_eq!(label, "myapp:prod — High error rate");
        assert!(
            !key.contains("rp006"),
            "key must not use the cloud project id"
        );
    }

    // ── Alert `key` contract: stable + discriminating ────────────────────────────

    fn loki_key(entry: &LokiEntry) -> String {
        match TaskOrigin::from_loki(entry) {
            TaskOrigin::Alert { key, .. } => key,
            other => panic!("expected Alert, got {other:?}"),
        }
    }

    #[test]
    fn loki_key_is_stable_across_volatile_fields() {
        // The same problem re-observed on a later scan: age, urgency, the
        // sampled log line, lookback, and url all differ — the key must not.
        let a = loki("myapp", "prod", "db timeout");
        let mut b = loki("myapp", "prod", "db timeout");
        b.age = chrono::Duration::hours(3);
        b.urgency = crate::urgency::Urgency::Low;
        b.line = r#"{"ts":"later"}"#.into();
        b.lookback = "1h".into();
        b.url = "https://grafana/d/changed".into();
        assert_eq!(loki_key(&a), loki_key(&b));
    }

    #[test]
    fn loki_key_discriminates_distinct_problems() {
        let base = loki("myapp", "prod", "db timeout");
        assert_ne!(
            loki_key(&base),
            loki_key(&loki("myapp", "prod", "oom killed"))
        );
        assert_ne!(
            loki_key(&base),
            loki_key(&loki("myapp", "staging", "db timeout"))
        );
        assert_ne!(
            loki_key(&base),
            loki_key(&loki("other", "prod", "db timeout"))
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
