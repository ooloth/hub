//! Fixtures shared by the TUI's tests.
use domain::pr_activity::{LatestVerdict, Opinion, Participant, PrActivity};

/// My pull request #7 on `ooloth/hub`, from `feat/thing` into `main`, opened just now, with no
/// body, reviews, CI result or activity. Tests that need other values set them with struct update.
pub(crate) fn pull_request() -> domain::PullRequest {
    domain::PullRequest {
        number: 7,
        title: "test pr".to_string(),
        repo: domain::RepoSlug::new("ooloth", "hub"),
        url: "https://github.com/ooloth/hub/pull/7".to_string(),
        age: chrono::Duration::zero(),
        urgency: domain::Urgency::Low,
        kind: domain::PrKind::Mine,
        draft: false,
        author: "ooloth".to_string(),
        review_decision: None,
        comment_count: 0,
        head_branch: "feat/thing".to_string(),
        base_branch: "main".to_string(),
        body: None,
        ci_status: None,
        changed_files: vec![],
        total_changed_files: 0,
        review_threads: vec![],
        pr_comments: vec![],
        merge_readiness: domain::MergeReadiness::NotYetKnown,
        activity: PrActivity::default(),
    }
}

/// Activity in which `approvals` reviewers approved and `changes_requested` others asked for
/// changes, each as their latest verdict.
pub(crate) fn verdicts(approvals: u32, changes_requested: u32) -> PrActivity {
    let verdict = |opinion: Opinion, n: u32| LatestVerdict {
        by: Participant::Person(format!("{opinion:?}-{n}")),
        opinion,
        at: chrono::DateTime::UNIX_EPOCH,
    };
    PrActivity {
        verdicts: (0..approvals)
            .map(|n| verdict(Opinion::Approved, n))
            .chain((0..changes_requested).map(|n| verdict(Opinion::ChangesRequested, n)))
            .collect(),
        ..PrActivity::default()
    }
}

/// Linear issue ENG-99, in progress, opened just now.
pub(crate) fn linear_issue() -> workflows::status::StatusItem {
    workflows::status::StatusItem::Linear(domain::LinearIssue {
        identifier: "ENG-99".to_string(),
        title: "Do the thing".to_string(),
        url: "https://linear.app/eng/issue/ENG-99".to_string(),
        state: "In Progress".to_string(),
        age: chrono::Duration::zero(),
        urgency: domain::Urgency::Medium,
    })
}

/// A media item whose import is blocked.
#[cfg(feature = "private")]
pub(crate) fn media_blocked() -> workflows::status::StatusItem {
    workflows::status::StatusItem::MediaBlocked(workflows::private::status::BlockedItem {
        source: "Media".to_string(),
        urgency: domain::Urgency::High,
        age: chrono::Duration::zero(),
        title: domain::UntrustedText::new("Item A"),
        error: domain::UntrustedText::new("Import failed"),
        url: "http://media-server/a".to_string(),
    })
}

/// A wanted media item that is missing.
#[cfg(feature = "private")]
pub(crate) fn media_missing() -> workflows::status::StatusItem {
    workflows::status::StatusItem::MediaMissing(workflows::private::status::MissingItem {
        source: "Media".to_string(),
        urgency: domain::Urgency::Medium,
        age: chrono::Duration::zero(),
        title: "Item B".to_string(),
        air_date: "2024-01-01".to_string(),
        url: "http://media-server/b".to_string(),
    })
}

/// A media source with items waiting in its backlog.
#[cfg(feature = "private")]
pub(crate) fn media_backlog() -> workflows::status::StatusItem {
    workflows::status::StatusItem::MediaBacklog {
        source: "Media".to_string(),
        count: 3,
    }
}

/// A media server's health warning.
#[cfg(feature = "private")]
pub(crate) fn media_health() -> workflows::status::StatusItem {
    workflows::status::StatusItem::MediaHealth(workflows::private::status::HealthItem {
        source: "Media".to_string(),
        urgency: domain::Urgency::Low,
        age: chrono::Duration::zero(),
        message: "Source unavailable".to_string(),
        url: "https://wiki.example.com/status".to_string(),
    })
}
