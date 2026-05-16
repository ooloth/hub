use domain::{PrKind, ReviewDecision};

use super::LaunchConfig;

const PROMPT: &str = include_str!("../../../../prompts/pr-review.md");

pub(crate) fn config(
    number: u64,
    repo: &str,
    kind: PrKind,
    author: &str,
    review_decision: Option<ReviewDecision>,
) -> LaunchConfig {
    let (skill, intent) = route(kind, review_decision, author);
    LaunchConfig {
        system_prompt: PROMPT.to_string(),
        prompt: format!("{skill}\n\nPR #{number} from {repo}. {intent}"),
        model: "opus".to_string(),
        allowed_tools: "Bash,Read,Edit,Write,Glob,Grep".to_string(),
        env: vec![],
    }
}

fn route(
    kind: PrKind,
    review_decision: Option<ReviewDecision>,
    author: &str,
) -> (&'static str, String) {
    match kind {
        PrKind::ToReview => (
            "/review-code",
            format!("This PR was authored by {author}. Your role is reviewer — do not make local changes."),
        ),
        PrKind::Mine | PrKind::MyDraft => {
            if review_decision == Some(ReviewDecision::ChangesRequested) {
                (
                    "/review-pr-comments-converge",
                    "This is your own PR and reviewers have requested changes. Address their feedback by making local changes.".to_string(),
                )
            } else {
                (
                    "/review-converge",
                    "This is your own PR. Review and improve it by making local changes.".to_string(),
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{config, route};
    use domain::{PrKind, ReviewDecision};

    #[test]
    fn pr_review_system_prompt_is_loaded() {
        let cfg = config(1, "ooloth/hub", PrKind::ToReview, "alice", None);
        assert!(cfg.system_prompt.contains("## Purpose"));
        assert!(!cfg.system_prompt.starts_with("---"));
    }

    #[test]
    fn to_review_routes_to_review_code() {
        let (skill, intent) = route(PrKind::ToReview, None, "alice");
        assert_eq!(skill, "/review-code");
        assert!(intent.contains("alice"));
        assert!(intent.contains("reviewer"));
    }

    #[test]
    fn mine_with_changes_requested_routes_to_review_pr_comments_converge() {
        let (skill, _) = route(PrKind::Mine, Some(ReviewDecision::ChangesRequested), "me");
        assert_eq!(skill, "/review-pr-comments-converge");
    }

    #[test]
    fn my_draft_with_changes_requested_routes_to_review_pr_comments_converge() {
        let (skill, _) = route(
            PrKind::MyDraft,
            Some(ReviewDecision::ChangesRequested),
            "me",
        );
        assert_eq!(skill, "/review-pr-comments-converge");
    }

    #[test]
    fn mine_without_changes_requested_routes_to_review_converge() {
        let (skill, _) = route(PrKind::Mine, None, "me");
        assert_eq!(skill, "/review-converge");
    }

    #[test]
    fn mine_approved_routes_to_review_converge() {
        let (skill, _) = route(PrKind::Mine, Some(ReviewDecision::Approved), "me");
        assert_eq!(skill, "/review-converge");
    }

    #[test]
    fn prompt_contains_pr_number_and_repo() {
        let cfg = config(42, "ooloth/hub", PrKind::ToReview, "bob", None);
        assert!(cfg.prompt.contains("42"));
        assert!(cfg.prompt.contains("ooloth/hub"));
    }
}
