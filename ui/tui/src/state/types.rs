use std::collections::HashSet;

use anyhow::Result;
use chrono::{DateTime, Utc};
use domain::{PrKind, PullRequest, ReviewDecision};
use workflows::status::{StatusItem, StatusReport};

use crate::display::{
    Category, DisplayItem, Filter, FlatRow, GroupKey, ListSnapshot, SelectedItemKind,
};

/// Which two-key submenu is currently intercepting keypresses. At most one can
/// be active at a time — mutually exclusive states represented as an enum.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum SubmenuState {
    #[default]
    None,
    PrActions,
    ReviewPicker,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReviewSkill {
    Converge,
    PrCommentsConverge,
}

impl ReviewSkill {
    pub(crate) const fn slash_command(self) -> &'static str {
        match self {
            Self::Converge => "/review-converge",
            Self::PrCommentsConverge => "/review-pr-comments-converge",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PrOwnership {
    Owned,
    External,
}

impl PrOwnership {
    pub(crate) const fn from_kind(kind: PrKind) -> Self {
        match kind {
            PrKind::Mine | PrKind::MyDraft => Self::Owned,
            PrKind::ToReview | PrKind::External => Self::External,
        }
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
        message: String,
        line: String,
        url: String,
        lookback: String,
        gcp_project: String,
    },
    LaunchLoki {
        project: String,
        env: String,
        title: String,
        message: String,
        line: String,
        url: String,
        lookback: String,
    },
    LaunchPr {
        repo: String,
        number: u64,
        kind: PrKind,
        author: String,
        review_decision: Option<ReviewDecision>,
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
    CommitReview(ReviewSkill),
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
        message: String,
        line: String,
        url: String,
        lookback: String,
        gcp_project: String,
    },
    LaunchLoki {
        project: String,
        env: String,
        title: String,
        message: String,
        line: String,
        url: String,
        lookback: String,
    },
    LaunchPr {
        repo: String,
        number: u64,
        kind: PrKind,
        review_decision: Option<ReviewDecision>,
        head_branch: String,
    },
    ReviewPr {
        repo: String,
        number: u64,
        ownership: PrOwnership,
        skill: ReviewSkill,
        head_branch: String,
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
