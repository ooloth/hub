use serde::{Deserialize, Serialize};

/// Priority ranking for any signal or task hub surfaces to the human.
/// Used across PRs, issues, CI failures, log alerts, and tasks.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum Urgency {
    Critical,
    High,
    Medium,
    Low,
}
