use crate::state::{PrAuthor, PrReview, PrReviewTarget};

use super::LaunchConfig;

/// A session opened on a PR with no task prompt and no skill.
///
/// `i` deliberately chooses nothing. Routing it by PR kind was tried and removed
/// in 9f402d6 for being too prescriptive; the picker behind `v` is the only place
/// a skill gets selected. An empty `prompt` is what makes `launch` omit the
/// positional argument entirely, so the session opens ready for a question.
pub(crate) fn ask_config(number: u64, repo: &str, author: PrAuthor) -> LaunchConfig {
    LaunchConfig {
        system_prompt: ask_prompt(number, repo, author),
        prompt: String::new(),
        supporting_data: None,
        model: "opus".to_string(),
        allowed_tools: "Bash,Read,Edit,Write,Glob,Grep".to_string(),
        env: vec![],
    }
}

fn ask_prompt(number: u64, repo: &str, author: PrAuthor) -> String {
    let hint = match author {
        PrAuthor::Me => "This is your PR. You can make local changes.",
        PrAuthor::Peer => {
            "This PR was authored by someone else. Post comments; do not make local changes."
        }
    };
    format!("PR #{number} ({repo}). {hint}")
}

pub(crate) fn review_config(target: &PrReviewTarget, review: PrReview) -> LaunchConfig {
    LaunchConfig {
        system_prompt: review_prompt(target.number, &target.repo, review),
        prompt: format!(
            "{} PR #{} ({})",
            review.slash_command(),
            target.number,
            target.repo
        ),
        supporting_data: None,
        model: "opus".to_string(),
        allowed_tools: "Bash,Read,Edit,Write,Glob,Grep".to_string(),
        env: vec![],
    }
}

fn review_prompt(number: u64, repo: &str, review: PrReview) -> String {
    let hint = match review {
        PrReview::ReviewMine => "This is your PR. Identify issues; make no local changes.",
        PrReview::FixMine => "This is your PR. Review and improve it by making local changes.",
        PrReview::AnswerReviewers => {
            "This is your PR. Reviewers have requested changes. Address their feedback with local changes."
        }
        PrReview::ReviewPeer => {
            "This PR was authored by someone else. Identify issues and post comments; make no local changes."
        }
    };
    format!("PR #{number} ({repo}). {hint}")
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::{ask_config, ask_prompt, review_config, review_prompt};
    use crate::state::{PrAuthor, PrReview, PrReviewTarget};

    fn target(author: PrAuthor) -> PrReviewTarget {
        PrReviewTarget {
            repo: "ooloth/hub".to_string(),
            number: 7,
            head_branch: "feature".to_string(),
            author,
        }
    }

    #[rstest]
    #[case(PrReview::ReviewMine, "/review-code")]
    #[case(PrReview::ReviewPeer, "/review-code")]
    #[case(PrReview::FixMine, "/review-converge")]
    #[case(PrReview::AnswerReviewers, "/review-pr-comments-converge")]
    fn review_config_prompt_leads_with_the_slash_command(
        #[case] review: PrReview,
        #[case] expected: &str,
    ) {
        let config = review_config(&target(review.author()), review);
        assert!(config.prompt.starts_with(expected), "{}", config.prompt);
    }

    #[test]
    fn review_config_prompt_names_the_pr_and_repo() {
        let config = review_config(&target(PrAuthor::Peer), PrReview::ReviewPeer);
        assert!(config.prompt.contains("#7"));
        assert!(config.prompt.contains("ooloth/hub"));
    }

    #[test]
    fn reviewing_a_peer_pr_forbids_local_changes() {
        let prompt = review_prompt(1, "r", PrReview::ReviewPeer);
        assert!(prompt.contains("someone else"));
        assert!(prompt.contains("no local changes"));
    }

    #[test]
    fn reading_my_own_pr_forbids_local_changes() {
        let prompt = review_prompt(1, "r", PrReview::ReviewMine);
        assert!(prompt.contains("your PR"));
        assert!(prompt.contains("no local changes"));
    }

    #[test]
    fn fixing_my_own_pr_invites_local_changes() {
        let prompt = review_prompt(1, "r", PrReview::FixMine);
        assert!(prompt.contains("your PR"));
        assert!(prompt.contains("making local changes"));
    }

    #[test]
    fn answering_reviewers_names_their_requested_changes() {
        let prompt = review_prompt(1, "r", PrReview::AnswerReviewers);
        assert!(prompt.contains("Reviewers have requested changes"));
    }

    /// `i` chooses no skill. Auto-routing it by PR kind was removed deliberately
    /// in 9f402d6 and drifted back in unnoticed; this is the check that stops it
    /// happening a third time.
    #[rstest]
    #[case(PrAuthor::Me)]
    #[case(PrAuthor::Peer)]
    fn ask_config_launches_no_skill(#[case] author: PrAuthor) {
        let config = ask_config(7, "ooloth/hub", author);
        assert!(
            config.prompt.is_empty(),
            "expected no task prompt, got {:?}",
            config.prompt
        );
        assert!(
            !config.system_prompt.contains("/review-"),
            "{}",
            config.system_prompt
        );
    }

    #[rstest]
    #[case(PrAuthor::Me)]
    #[case(PrAuthor::Peer)]
    fn ask_prompt_names_the_pr_and_repo(#[case] author: PrAuthor) {
        let prompt = ask_prompt(7, "ooloth/hub", author);
        assert!(prompt.contains("#7"));
        assert!(prompt.contains("ooloth/hub"));
    }

    #[test]
    fn asking_about_a_peer_pr_forbids_local_changes() {
        let prompt = ask_prompt(7, "ooloth/hub", PrAuthor::Peer);
        assert!(prompt.contains("someone else"));
        assert!(prompt.contains("do not make local changes"));
    }

    #[test]
    fn every_review_prompt_names_the_pr_number_and_repo() {
        for review in [
            PrReview::ReviewMine,
            PrReview::FixMine,
            PrReview::AnswerReviewers,
            PrReview::ReviewPeer,
        ] {
            let prompt = review_prompt(42, "ooloth/hub", review);
            assert!(prompt.contains("#42"), "{review:?}");
            assert!(prompt.contains("ooloth/hub"), "{review:?}");
        }
    }
}
