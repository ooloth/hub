use std::collections::HashSet;

use anyhow::Result;
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
    },
    IssueDetail {
        parent: ListSnapshot,
        issue: Issue,
        scroll: u16,
    },
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
    ReviewingPr {
        parent: ListSnapshot,
        pr: PullRequest,
    },
    MergingPr {
        parent: ListSnapshot,
        pr: PullRequest,
    },
    DismissingIssue {
        parent: ListSnapshot,
        issue: Issue,
        input: tui_input::Input,
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
        }
    }
}

pub(crate) enum EnterAction {
    None,
    OpenUrl(String),
    OpenLogDetail(LogDetailView),
    OpenIssueDetail(Issue),
    OpenPrDetail(PullRequest),
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
    CommitMerge,
    CancelMerge,
    DismissIssue,
    DismissInput(tui_input::InputRequest),
    CommitDismissal,
    CancelDismissal,
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
}
