mod ci;
mod issues;
mod prs;

pub use ci::ci_failures;
pub use issues::{dismiss_issue, issue_states, issues, set_issue_labels};
pub use prs::{
    external_prs, merge_pull_request, my_draft_prs, my_open_prs, pr_states, prs_awaiting_review,
};

use chrono::Utc;

fn age(created_at: &str) -> chrono::Duration {
    let Ok(created) = chrono::DateTime::parse_from_rfc3339(created_at) else {
        return chrono::Duration::zero();
    };
    let d = Utc::now() - created.to_utc();
    if d < chrono::Duration::zero() {
        chrono::Duration::zero()
    } else {
        d
    }
}
