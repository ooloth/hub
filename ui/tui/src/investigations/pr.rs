use crate::state::{PrOwnership, ReviewSkill};

use super::LaunchConfig;

pub(crate) fn bare_config(number: u64, repo: &str, ownership: PrOwnership) -> LaunchConfig {
    LaunchConfig {
        system_prompt: system_prompt(number, repo, ownership, None),
        prompt: String::new(),
        supporting_data: None,
        model: "opus".to_string(),
        allowed_tools: "Bash,Read,Edit,Write,Glob,Grep".to_string(),
        env: vec![],
    }
}

pub(crate) fn review_config(
    number: u64,
    repo: &str,
    ownership: PrOwnership,
    skill: ReviewSkill,
) -> LaunchConfig {
    LaunchConfig {
        system_prompt: system_prompt(number, repo, ownership, Some(skill)),
        prompt: format!("{} PR #{number} ({repo})", skill.slash_command()),
        supporting_data: None,
        model: "opus".to_string(),
        allowed_tools: "Bash,Read,Edit,Write,Glob,Grep".to_string(),
        env: vec![],
    }
}

fn system_prompt(
    number: u64,
    repo: &str,
    ownership: PrOwnership,
    skill: Option<ReviewSkill>,
) -> String {
    let hint = match (ownership, skill) {
        (PrOwnership::Owned, None) => {
            "This is your PR. You can make local changes.".to_string()
        }
        (PrOwnership::External, None) => {
            "This PR was authored by someone else. Post comments; do not make local changes."
                .to_string()
        }
        (PrOwnership::Owned, Some(ReviewSkill::Converge)) => {
            "This is your PR. Review and improve it by making local changes.".to_string()
        }
        (PrOwnership::External, Some(ReviewSkill::Converge)) => {
            "This PR was authored by someone else. Your role is reviewer — write a PR comment."
                .to_string()
        }
        (PrOwnership::Owned, Some(ReviewSkill::PrCommentsConverge)) => {
            "This is your PR. Reviewers have requested changes. Address their feedback with local changes.".to_string()
        }
        (PrOwnership::External, Some(ReviewSkill::PrCommentsConverge)) => {
            "This PR was authored by someone else. Respond to reviewer feedback with your own review comments.".to_string()
        }
    };
    format!("PR #{number} ({repo}). {hint}")
}

#[cfg(test)]
mod tests {
    use super::{bare_config, review_config, system_prompt};
    use crate::state::{PrOwnership, ReviewSkill};

    #[test]
    fn bare_config_has_empty_prompt() {
        let cfg = bare_config(42, "ooloth/hub", PrOwnership::Owned);
        assert_eq!(cfg.prompt, "");
    }

    #[test]
    fn bare_owned_system_prompt_contains_pr_number_repo_and_owned_hint() {
        let cfg = bare_config(42, "ooloth/hub", PrOwnership::Owned);
        assert!(cfg.system_prompt.contains("42"));
        assert!(cfg.system_prompt.contains("ooloth/hub"));
        assert!(cfg.system_prompt.contains("your PR"));
        assert!(cfg.system_prompt.contains("local changes"));
    }

    #[test]
    fn bare_external_system_prompt_contains_post_comments_hint() {
        let cfg = bare_config(42, "ooloth/hub", PrOwnership::External);
        assert!(cfg.system_prompt.contains("Post comments"));
        assert!(cfg.system_prompt.contains("do not make local changes"));
    }

    #[test]
    fn review_config_prompt_contains_skill_and_pr_number() {
        let cfg = review_config(7, "ooloth/hub", PrOwnership::Owned, ReviewSkill::Converge);
        assert!(cfg.prompt.contains("/review-converge"));
        assert!(cfg.prompt.contains("7"));
        assert!(cfg.prompt.contains("ooloth/hub"));
    }

    #[test]
    fn review_config_pr_comments_prompt_contains_correct_skill() {
        let cfg = review_config(
            7,
            "ooloth/hub",
            PrOwnership::Owned,
            ReviewSkill::PrCommentsConverge,
        );
        assert!(cfg.prompt.contains("/review-pr-comments-converge"));
    }

    #[test]
    fn owned_converge_system_prompt_says_improve() {
        let s = system_prompt(1, "r", PrOwnership::Owned, Some(ReviewSkill::Converge));
        assert!(s.contains("your PR"));
        assert!(s.contains("local changes"));
    }

    #[test]
    fn external_converge_system_prompt_says_write_pr_comment() {
        let s = system_prompt(1, "r", PrOwnership::External, Some(ReviewSkill::Converge));
        assert!(s.contains("someone else"));
        assert!(s.contains("PR comment"));
    }

    #[test]
    fn owned_pr_comments_system_prompt_says_address_feedback() {
        let s = system_prompt(
            1,
            "r",
            PrOwnership::Owned,
            Some(ReviewSkill::PrCommentsConverge),
        );
        assert!(s.contains("Reviewers have requested changes"));
    }

    #[test]
    fn external_pr_comments_system_prompt_says_respond_to_reviewer() {
        let s = system_prompt(
            1,
            "r",
            PrOwnership::External,
            Some(ReviewSkill::PrCommentsConverge),
        );
        assert!(s.contains("someone else"));
        assert!(s.contains("review comments"));
    }
}
