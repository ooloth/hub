//! What a window hub opens on a pull request does there.

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

/// What a window on a pull request is for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrWindowActivity {
    /// A Claude session with no skill, opened ready for a question.
    Investigate,
    /// A Claude session running one review skill.
    Review(ReviewSkill),
    /// lazygit in the pull request's worktree.
    Lazygit,
    /// neovim's Octo on the pull request.
    Octo,
    /// The pull request's diff.
    Diff,
}

impl PrWindowActivity {
    /// The kind segment of the tmux name.
    ///
    /// An investigation and every review share `pr`, so in tmux they share one window, as they
    /// always have. Rex gives each its own tab through [`Self::label`].
    #[must_use]
    pub(crate) const fn tmux_kind(self) -> &'static str {
        match self {
            Self::Investigate | Self::Review(_) => "pr",
            Self::Lazygit => "git",
            Self::Octo => "octo",
            Self::Diff => "diff",
        }
    }

    /// The activity a Rex label ends in.
    #[must_use]
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Investigate => "investigate",
            Self::Review(skill) => skill.name(),
            Self::Lazygit => "lazygit",
            Self::Octo => "octo",
            Self::Diff => "diff",
        }
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
