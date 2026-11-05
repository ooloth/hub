//! The review skills hub can start on a pull request.

/// A review skill hub can start on a pull request.
///
/// The skill's name is the only place it is spelled: the slash command a review sends and the
/// activity its Rex label names both come from [`Self::name`], so `monitor-prs` sees the skill
/// the tab actually runs (decision 033).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReviewSkill {
    /// `review-code`: find issues without changing anything.
    ReviewCode,
    /// `review-converge`: review and fix in rounds.
    ReviewConverge,
    /// `review-pr-comments-converge`: answer reviewers' comments.
    ReviewPrCommentsConverge,
}

impl ReviewSkill {
    /// The skill's name, as its slash command and a tab label spell it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::ReviewCode => "review-code",
            Self::ReviewConverge => "review-converge",
            Self::ReviewPrCommentsConverge => "review-pr-comments-converge",
        }
    }

    /// The slash command that starts the skill.
    #[must_use]
    pub fn slash_command(self) -> String {
        format!("/{}", self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::ReviewSkill;
    use rstest::rstest;

    #[rstest]
    #[case(ReviewSkill::ReviewCode, "/review-code")]
    #[case(ReviewSkill::ReviewConverge, "/review-converge")]
    #[case(ReviewSkill::ReviewPrCommentsConverge, "/review-pr-comments-converge")]
    fn a_skills_slash_command_is_its_name(#[case] skill: ReviewSkill, #[case] command: &str) {
        assert_eq!(skill.slash_command(), command);
    }
}
