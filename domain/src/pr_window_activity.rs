//! What a window hub opens on a pull request does there.

use crate::ReviewSkill;

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
