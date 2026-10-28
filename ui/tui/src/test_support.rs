//! Fixtures shared by the TUI's tests.
use domain::pr_activity::{LatestVerdict, Opinion, Participant, PrActivity};

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
