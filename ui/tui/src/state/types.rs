use std::collections::HashSet;

use anyhow::Result;
use chrono::{DateTime, Utc};
use domain::{PrKind, PullRequest, RepoSlug, ReviewDecision, TaskId, TaskStatus};
use workflows::status::{StatusItem, StatusReport};

use crate::display::{Category, DisplayItem, Filter, FlatRow, GroupKey, ListSnapshot};

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

/// Whether the UnifiedList is showing a split detail pane below the list, and
/// which content is shown. `detail_scroll` only exists inside the visible
/// variants — a hidden pane cannot have a stale scroll offset.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) enum DetailMode {
    #[default]
    Hidden,
    /// Split view is open showing the selected signal's own detail (PR body,
    /// issue body, CI log, session transcript for task rows, etc.).
    Visible { detail_scroll: u16 },
    /// Split view is open and toggled to show the agent session detail for the
    /// attached task of a `BadgedSignal` row. Only reachable when the selected
    /// row is a `DisplayItem::BadgedSignal`.
    VisibleSession { detail_scroll: u16 },
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
                FlatRow::BadgedSignal { item, .. } => Some(item.clone()),
            },
            Screen::MergingPr { pr, .. } => Some(StatusItem::Pr(pr.clone())),
        }
    }

    /// Like `selected_status_item`, but returns the `first_item` of a `GroupHeader`
    /// rather than `None` — used to seed the task creation form from grouped rows.
    pub(crate) fn selected_item_for_seeding(&self) -> Option<StatusItem> {
        match self {
            Screen::UnifiedList {
                flat_rows,
                selected,
                ..
            } => match flat_rows.get(*selected)? {
                FlatRow::Single(item) => Some(item.clone()),
                FlatRow::GroupChild { item, .. } => Some(item.clone()),
                FlatRow::GroupHeader { first_item, .. } => Some(first_item.clone()),
                FlatRow::BadgedSignal { item, .. } => Some(item.clone()),
            },
            Screen::MergingPr { pr, .. } => Some(StatusItem::Pr(pr.clone())),
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
    TaskStatusSubmenu,
    CancelTaskStatusSubmenu,
    TransitionTaskStatus(TaskStatus),
    // Task creation modal
    OpenTaskCreationForm,
    OpenBlankTaskCreationForm,
    CancelTaskCreation,
    FocusNextField,
    FocusPrevField,
    CycleTaskKind,
    ModalTextInput(crossterm::event::KeyEvent),
    CommitTaskCreation,
    ScrollDetailDown,
    ScrollDetailUp,
    ToggleSessionDetail,
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
    UpdateTaskStatus {
        id: TaskId,
        status: TaskStatus,
    },
    CreateTask {
        title: String,
        description: Option<String>,
        kind: domain::TaskKind,
        links: Vec<String>,
        repo: Option<RepoSlug>,
        origin: domain::TaskOrigin,
    },
}

pub(crate) enum Msg {
    Action(Action),
    Tick,
    FetchResult(Result<StatusReport>),
    AppliedFromCache {
        report: StatusReport,
        refreshed_at: DateTime<Utc>,
    },
    StreamUpdate(Vec<domain::StreamBlock>),
    /// Fresh task list from the 10s poll, patched into the in-memory items so
    /// status changes surface without waiting for the full external refresh.
    TasksPatched(Vec<domain::Task>),
}
