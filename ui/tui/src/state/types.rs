use std::collections::HashSet;

use anyhow::Result;
use chrono::{DateTime, Utc};
use domain::{Issue, PrKind, PullRequest, ReviewDecision};
use workflows::status::{StatusItem, StatusReport};

use crate::display::{
    Category, DisplayItem, Filter, FlatRow, GroupKey, ListSnapshot, LogDetailView,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReviewSkill {
    Converge,
    PrCommentsConverge,
}

impl ReviewSkill {
    pub(crate) fn slash_command(self) -> &'static str {
        match self {
            ReviewSkill::Converge => "/review-converge",
            ReviewSkill::PrCommentsConverge => "/review-pr-comments-converge",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PrOwnership {
    Owned,
    External,
}

impl PrOwnership {
    pub(crate) fn from_kind(kind: PrKind) -> Self {
        match kind {
            PrKind::Mine | PrKind::MyDraft => PrOwnership::Owned,
            PrKind::ToReview | PrKind::External => PrOwnership::External,
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

/// Whether the UnifiedList is showing a split detail pane below the list.
/// `detail_scroll` only exists inside `Visible` — a hidden pane cannot have
/// a stale scroll offset.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) enum DetailMode {
    #[default]
    Hidden,
    Visible {
        detail_scroll: u16,
    },
}

// Flat enum encoding the valid navigation graph in the type system.
// UnifiedList is the default (top-level) screen. Detail screens carry a
// ListSnapshot as their return address — pressing Back restores the list
// to the exact items/selection/filter/expansion state before drill-in.
// IssueDetail shows the full body of a single issue with a scroll offset.
// LogDetail shows the raw JSON log line for a GCP or Loki entry.
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
    IssueDetail {
        parent: ListSnapshot,
        issue: Issue,
        scroll: u16,
    },
    // Entry point removed in #244 (split view replaced full-screen navigation).
    // Kept for potential future use (e.g. deeper drill-in from split detail pane).
    #[allow(dead_code)]
    LogDetail {
        parent: ListSnapshot,
        view: LogDetailView,
        scroll: u16,
    },
    PrDetail {
        parent: ListSnapshot,
        pr: PullRequest,
        scroll: u16,
    },
    PrSplit {
        parent: ListSnapshot,
        all_items: Vec<PullRequest>,
        items: Vec<PullRequest>,
        selected: usize,
        query: Option<String>,
    },
    ReviewingPr {
        parent: ListSnapshot,
        pr: PullRequest,
        prev: PrPrevScreen,
    },
    MergingPr {
        parent: ListSnapshot,
        pr: PullRequest,
        prev: PrPrevScreen,
    },
    DismissingIssue {
        parent: ListSnapshot,
        issue: Issue,
        input: tui_input::Input,
    },
}

/// The screen a Review/Merge picker should restore on commit or cancel.
///
/// Pickers are always entered from either PrDetail or PrSplit. The
/// variant records just enough to rebuild the source screen — for
/// PrSplit, the full PR list and selection so the user lands back on
/// the same card they were viewing.
#[derive(Clone, Debug)]
pub(crate) enum PrPrevScreen {
    /// Restore Screen::PrDetail with scroll = 0 (matches the prior
    /// behaviour before this enum was introduced).
    PrDetail,
    PrSplit {
        all_items: Vec<PullRequest>,
        items: Vec<PullRequest>,
        selected: usize,
        query: Option<String>,
    },
}

impl Default for Screen {
    fn default() -> Self {
        Screen::UnifiedList {
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
            Screen::UnifiedList {
                flat_rows,
                selected,
                ..
            } => match flat_rows.get(*selected)? {
                FlatRow::Single(item) => Some(item.clone()),
                FlatRow::GroupChild { item, .. } => Some(item.clone()),
                FlatRow::GroupHeader { .. } => None,
            },
            Screen::LogDetail { .. } => None,
            Screen::IssueDetail { issue, .. } | Screen::DismissingIssue { issue, .. } => {
                Some(StatusItem::Issue(issue.clone()))
            }
            Screen::PrDetail { pr, .. }
            | Screen::ReviewingPr { pr, .. }
            | Screen::MergingPr { pr, .. } => Some(StatusItem::Pr(pr.clone())),
            Screen::PrSplit {
                items, selected, ..
            } => items.get(*selected).cloned().map(StatusItem::Pr),
        }
    }
}

pub(crate) enum EnterAction {
    None,
    OpenLogDetail,
    OpenIssueDetail,
    OpenPrDetail,
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
    AskAboutPr,
    OpenReviewPicker,
    CommitReview(ReviewSkill),
    CancelReview,
    Refresh,
    ApproveForAgent,
    MergePr,
    OpenInOcto,
    OpenInLazygit,
    CommitMerge,
    CancelMerge,
    DismissIssue,
    DismissInput(tui_input::InputRequest),
    CommitDismissal,
    CancelDismissal,
    ScrollDetailDown,
    ScrollDetailUp,
    // Filter actions — only take effect from UnifiedList in normal mode.
    FilterCategory(Category),
    EnterPrSplit,
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
    SetIssueLabels {
        repo: String,
        number: u64,
        labels: Vec<String>,
    },
    MergePullRequest {
        repo: String,
        number: u64,
    },
    DismissIssue {
        repo: String,
        number: u64,
        reason: String,
        labels: Vec<String>,
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
    AskAboutPr {
        repo: String,
        number: u64,
        ownership: PrOwnership,
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
