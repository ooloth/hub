use std::collections::HashSet;

use anyhow::Result;
use chrono::{DateTime, Utc};
use domain::{PrKind, PullRequest, UntrustedText};
use workflows::status::{StatusItem, StatusReport};

use crate::display::{
    Category, DisplayItem, Filter, FlatRow, GroupKey, ListSnapshot, SelectedItemKind,
};

/// Which two-key submenu is currently intercepting keypresses. At most one can
/// be active at a time — mutually exclusive states represented as an enum.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) enum SubmenuState {
    #[default]
    None,
    PrActions,
    /// The review picker, holding the PR it was opened for. Capturing the target
    /// at open time keeps a background refresh from swapping the PR out from
    /// under an armed picker — the same reason `Screen::MergingPr` snapshots its
    /// `PullRequest`.
    ReviewPicker(PrReviewTarget),
}

/// Who wrote a PR, from the current user's point of view.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PrAuthor {
    Me,
    Peer,
}

impl PrAuthor {
    pub(crate) const fn from_kind(kind: PrKind) -> Self {
        match kind {
            PrKind::Mine | PrKind::MyDraft => Self::Me,
            PrKind::ToReview | PrKind::External => Self::Peer,
        }
    }

    /// The review sessions offered for a PR with this author, in picker order.
    ///
    /// Single source of truth for both the status-bar label and the key handler,
    /// so the bar cannot advertise a key the handler ignores.
    pub(crate) fn review_options(self) -> &'static [ReviewOption] {
        const MINE: &[ReviewOption] = &[
            ReviewOption {
                key: 'c',
                review: PrReview::ReviewMine,
                label: "code",
            },
            ReviewOption {
                key: 'f',
                review: PrReview::FixMine,
                label: "fix",
            },
            ReviewOption {
                key: 'm',
                review: PrReview::AnswerReviewers,
                label: "comments",
            },
        ];

        // A peer's PR offers no review that edits their branch.
        const PEER: &[ReviewOption] = &[ReviewOption {
            key: 'c',
            review: PrReview::ReviewPeer,
            label: "code",
        }];

        let options = match self {
            Self::Me => MINE,
            Self::Peer => PEER,
        };

        assert!(
            !options.is_empty(),
            "review picker offers no option for this author"
        );
        assert!(
            options
                .iter()
                .enumerate()
                .all(|(i, option)| options.iter().skip(i + 1).all(|o| o.key != option.key)),
            "review picker options share a key"
        );
        assert!(
            options.iter().all(|option| option.review.author() == self),
            "review picker offers a review meant for a different author"
        );
        options
    }
}

/// What a review session does to a PR.
///
/// Flat rather than an author-by-mode pair so that a peer's PR combined with a
/// fixing skill is not a value anyone can write.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PrReview {
    /// My PR: read it critically, change nothing.
    ReviewMine,
    /// My PR: review and fix it.
    FixMine,
    /// My PR: work through what reviewers said.
    AnswerReviewers,
    /// Someone else's PR: read it critically, change nothing.
    ReviewPeer,
}

impl PrReview {
    pub(crate) const fn slash_command(self) -> &'static str {
        match self {
            Self::ReviewMine | Self::ReviewPeer => "/review-code",
            Self::FixMine => "/review-converge",
            Self::AnswerReviewers => "/review-pr-comments-converge",
        }
    }

    pub(crate) const fn author(self) -> PrAuthor {
        match self {
            Self::ReviewMine | Self::FixMine | Self::AnswerReviewers => PrAuthor::Me,
            Self::ReviewPeer => PrAuthor::Peer,
        }
    }
}

/// One entry in the review picker: the key that selects it, what it launches,
/// and how the status bar names it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ReviewOption {
    pub(crate) key: char,
    pub(crate) review: PrReview,
    pub(crate) label: &'static str,
}

/// The PR a review picker was opened for, captured at the moment it opens.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PrReviewTarget {
    pub(crate) repo: String,
    pub(crate) number: u64,
    pub(crate) head_branch: String,
    pub(crate) author: PrAuthor,
}

impl PrReviewTarget {
    pub(crate) fn from_pr(pr: &PullRequest) -> Self {
        Self {
            repo: pr.repo.to_string(),
            number: pr.number,
            head_branch: pr.head_branch.clone(),
            author: PrAuthor::from_kind(pr.kind),
        }
    }
}

#[cfg(test)]
mod review_option_tests {
    use rstest::rstest;

    use super::{PrAuthor, PrReview};

    #[test]
    fn peer_pr_offers_only_a_read_only_review() {
        let options = PrAuthor::Peer.review_options();
        let offered: Vec<_> = options.iter().map(|o| (o.key, o.review)).collect();
        assert_eq!(offered, vec![('c', PrReview::ReviewPeer)]);
    }

    #[test]
    fn my_pr_offers_read_only_fix_and_reviewer_replies() {
        let options = PrAuthor::Me.review_options();
        let offered: Vec<_> = options.iter().map(|o| (o.key, o.review)).collect();
        assert_eq!(
            offered,
            vec![
                ('c', PrReview::ReviewMine),
                ('f', PrReview::FixMine),
                ('m', PrReview::AnswerReviewers),
            ]
        );
    }

    #[rstest]
    #[case(PrAuthor::Me)]
    #[case(PrAuthor::Peer)]
    fn every_offered_review_belongs_to_the_author_it_is_offered_for(#[case] author: PrAuthor) {
        for option in author.review_options() {
            assert_eq!(option.review.author(), author, "key {}", option.key);
        }
    }

    #[rstest]
    #[case(PrReview::ReviewMine, "/review-code")]
    #[case(PrReview::ReviewPeer, "/review-code")]
    #[case(PrReview::FixMine, "/review-converge")]
    #[case(PrReview::AnswerReviewers, "/review-pr-comments-converge")]
    fn slash_command_names_the_skill_for_each_review(
        #[case] review: PrReview,
        #[case] expected: &str,
    ) {
        assert_eq!(review.slash_command(), expected);
    }
}

#[derive(Debug, Default)]
pub(crate) enum RefreshState {
    #[default]
    Idle,
    InProgress,
    /// Partial refresh: some sources succeeded, others failed.
    Partial(Vec<String>),
    Failed(String),
}

/// Whether the `UnifiedList` is showing a split detail pane below the list.
/// `detail_scroll` only exists inside the visible variant — a hidden pane cannot
/// have a stale scroll offset.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum DetailMode {
    #[default]
    Hidden,
    /// Split view is open showing the selected signal's detail (PR body,
    /// issue body, CI log, etc.).
    Visible { detail_scroll: u16 },
}

#[derive(Debug)]
pub(crate) enum Screen {
    UnifiedList {
        items: Vec<DisplayItem>,
        flat_rows: Vec<FlatRow>,
        selected: usize,
        filter: Filter,
        expanded_groups: HashSet<GroupKey>,
        detail_mode: DetailMode,
    },
    MergingPr {
        parent: ListSnapshot,
        pr: PullRequest,
        prev: PrPrevScreen,
    },
}

/// The screen the merge picker restores on commit or cancel.
#[derive(Clone, Debug)]
pub(crate) enum PrPrevScreen {
    UnifiedList { snapshot: ListSnapshot },
}

impl Default for Screen {
    fn default() -> Self {
        Self::UnifiedList {
            items: vec![],
            flat_rows: vec![],
            selected: 0,
            filter: Filter::default(),
            expanded_groups: HashSet::new(),
            detail_mode: DetailMode::Hidden,
        }
    }
}

impl Screen {
    pub(crate) fn selected_status_item(&self) -> Option<StatusItem> {
        match self {
            Self::UnifiedList {
                flat_rows,
                selected,
                ..
            } => match flat_rows.get(*selected)? {
                FlatRow::Single(item) | FlatRow::GroupChild { item, .. } => Some(item.clone()),
                FlatRow::GroupHeader { .. } => None,
            },
            Self::MergingPr { pr, .. } => Some(StatusItem::Pr(pr.clone())),
        }
    }

    /// Returns the `SelectedItemKind` for the currently selected row.
    pub(crate) fn selected_item_kind(&self) -> SelectedItemKind {
        match self {
            Self::UnifiedList {
                flat_rows,
                selected,
                ..
            } => flat_rows
                .get(*selected)
                .map_or(SelectedItemKind::Other, SelectedItemKind::from_row),
            Self::MergingPr { .. } => SelectedItemKind::Other,
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum InvestigateAction {
    None,
    LaunchCi {
        repo: String,
        workflow: String,
        run_url: String,
    },
    LaunchIssue {
        repo: String,
        number: u64,
    },
    LaunchGcp {
        project: String,
        env: String,
        title: String,
        message: UntrustedText,
        line: UntrustedText,
        url: String,
        lookback: String,
        gcp_project: String,
    },
    LaunchLoki {
        project: String,
        env: String,
        title: String,
        message: UntrustedText,
        line: UntrustedText,
        url: String,
        lookback: String,
    },
    LaunchPr {
        repo: String,
        number: u64,
        kind: PrKind,
        author: String,
        head_branch: String,
        base_branch: String,
    },
    #[cfg(feature = "private")]
    LaunchMediaBlocked {
        title: String,
        error: String,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Action {
    Quit,
    ToggleHelp,
    CloseHelp,
    Back,
    MoveUp,
    MoveDown,
    MoveToTop,
    MoveToBottom,
    MovePageUp,
    MovePageDown,
    PendingG,
    Enter,
    ExpandGroup,
    CollapseGroup,
    Investigate,
    OpenReviewPicker,
    CommitReview(PrReview),
    CancelReview,
    OpenUrl,
    PrActionSubmenu,
    CancelPrSubmenu,
    OpenPrDiffInDelta,
    Refresh,
    ApproveForAgent,
    MergePr,
    OpenInOcto,
    OpenInLazygit,
    CommitMerge,
    CancelMerge,
    ScrollDetailDown,
    ScrollDetailUp,
    // Filter actions — only take effect from UnifiedList in normal mode.
    FilterCategory(Category),
    ClearFilter,
    StartQuery,
    AppendQuery(char),
    BackspaceQuery,
    CommitQuery,
    CancelQuery,
}

pub(crate) enum Effect {
    Quit,
    OpenUrl(String),
    OpenPrDiffInDelta {
        repo: String,
        number: u64,
    },
    SetIssueLabels {
        repo: String,
        number: u64,
        labels: Vec<String>,
    },
    MergePullRequest {
        repo: String,
        number: u64,
    },
    LaunchCi {
        repo: String,
        workflow: String,
        run_url: String,
    },
    LaunchIssue {
        repo: String,
        number: u64,
    },
    LaunchGcp {
        project: String,
        env: String,
        title: String,
        message: UntrustedText,
        line: UntrustedText,
        url: String,
        lookback: String,
        gcp_project: String,
    },
    LaunchLoki {
        project: String,
        env: String,
        title: String,
        message: UntrustedText,
        line: UntrustedText,
        url: String,
        lookback: String,
    },
    LaunchPr {
        repo: String,
        number: u64,
        author: PrAuthor,
        head_branch: String,
    },
    ReviewPr {
        target: PrReviewTarget,
        review: PrReview,
    },
    OpenInOcto {
        repo: String,
        number: u64,
        head_branch: String,
    },
    OpenInLazygit {
        repo: String,
        number: u64,
        head_branch: String,
    },
    #[cfg(feature = "private")]
    LaunchMediaBlocked {
        title: String,
        error: String,
    },
    StartRefresh,
    WriteCache(String),
}

pub(crate) enum Msg {
    Action(Action),
    Tick,
    FetchResult(Result<StatusReport>),
    AppliedFromCache {
        report: StatusReport,
        refreshed_at: DateTime<Utc>,
    },
}
