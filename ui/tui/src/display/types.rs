use std::collections::HashSet;

use domain::Task;
use workflows::status::StatusItem;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct GroupKey(String);

impl GroupKey {
    pub(crate) const fn new(s: String) -> Self {
        Self(s)
    }
}

impl std::fmt::Display for GroupKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug)]
pub(crate) enum FlatRow {
    Single(StatusItem),
    GroupHeader {
        key: GroupKey,
        count: usize,
        urgency: domain::Urgency,
        expanded: bool,
        first_item: StatusItem,
    },
    GroupChild {
        #[allow(dead_code)]
        parent_key: GroupKey,
        item: StatusItem,
        is_last: bool,
    },
    /// A signal row with an attached in-progress task badge.
    BadgedSignal {
        item: StatusItem,
        task: Task,
    },
}

impl FlatRow {
    pub(crate) const fn attached_task(&self) -> Option<&Task> {
        match self {
            Self::BadgedSignal { task, .. } => Some(task),
            _ => None,
        }
    }
}

/// A single log line, parsed once at construction.
#[derive(Clone, Debug)]
pub(crate) enum LogLine {
    Json(serde_json::Value),
    Raw(String),
}

impl LogLine {
    pub(crate) fn parse(s: &str) -> Self {
        serde_json::from_str(s).map_or_else(|_| Self::Raw(s.to_string()), Self::Json)
    }
}

/// View data for the `LogDetail` screen: shared metadata from one monitoring alert
/// plus all raw log lines from that alert window (one for a single item, many for a group).
#[derive(Clone, Debug)]
pub(crate) enum LogDetailView {
    Gcp {
        project: String,
        env: String,
        title: String,
        message: String,
        lines: Vec<LogLine>,
    },
    Loki {
        project: String,
        env: String,
        title: String,
        message: String,
        lines: Vec<LogLine>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub(crate) enum Category {
    Prs,
    Issues,
    Errors,
    Tasks,
}

impl Category {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Prs => "PRs",
            Self::Issues => "Issues",
            Self::Errors => "Errors",
            Self::Tasks => "Tasks",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) enum DisplayItem {
    Single(StatusItem),
    Group {
        label: GroupKey,
        items: Vec<StatusItem>,
    },
    /// A signal row with an attached in-progress task. The task row is
    /// suppressed and replaced by this badge on the signal row.
    BadgedSignal {
        signal: StatusItem,
        task: Task,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct ListSnapshot {
    pub(crate) items: Vec<DisplayItem>,
    pub(crate) selected: usize,
    pub(crate) filter: Filter,
    pub(crate) expanded_groups: HashSet<GroupKey>,
    pub(crate) detail_mode: crate::state::DetailMode,
}

#[derive(Clone, Debug)]
pub(crate) enum RowSeparator {
    Bullet,
    Toggle(bool),
    TreeChild(bool), // true = last child (renders └), false = non-last (renders │)
}

#[derive(Clone, Debug)]
pub(crate) struct LineParts {
    pub(crate) separator: RowSeparator,
    pub(crate) primary: Vec<String>,
    pub(crate) dim_inline: Vec<String>,
    pub(crate) source: Option<String>,
    pub(crate) category: String,
    pub(crate) age: String,
}

impl LineParts {
    pub(crate) fn flat(&self) -> String {
        self.primary.join(" · ") + &self.dim_inline.join(" · ")
    }

    pub(crate) fn all_text(&self) -> String {
        let mut parts: Vec<&str> = vec![&self.category];
        parts.extend(self.primary.iter().map(String::as_str));
        parts.extend(self.dim_inline.iter().map(String::as_str));
        if let Some(s) = &self.source {
            parts.push(s);
        }
        parts.push(&self.age);
        parts.join(" ")
    }
}

/// Which broad category the selected item belongs to — used by the input
/// layer to decide which context-specific keybindings to emit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SelectedItemKind {
    Pr,
    Issue,
    Task,
    /// A signal row (PR, Issue, CI, etc.) that has an attached active task.
    /// The `s` key opens the task status submenu on these rows.
    BadgedSignal,
    Other,
}

impl SelectedItemKind {
    pub(crate) const fn from_item(item: &StatusItem) -> Self {
        match item {
            StatusItem::Pr(_) => Self::Pr,
            StatusItem::Issue(_) => Self::Issue,
            StatusItem::AgentSession(_) => Self::Task,
            _ => Self::Other,
        }
    }

    pub(crate) const fn from_row(row: &FlatRow) -> Self {
        match row {
            FlatRow::BadgedSignal { .. } => Self::BadgedSignal,
            FlatRow::Single(item) | FlatRow::GroupChild { item, .. } => Self::from_item(item),
            FlatRow::GroupHeader { .. } => Self::Other,
        }
    }
}

pub(crate) enum InvestigationKind {
    Ci {
        repo: String,
        run_url: String,
    },
    Issue {
        repo: String,
        number: u64,
    },
    Gcp {
        project: String,
        env: String,
        title: String,
        message: String,
        line: String,
        url: String,
        lookback: String,
        gcp_project: String,
    },
    Loki {
        project: String,
        env: String,
        title: String,
        message: String,
        line: String,
        url: String,
        lookback: String,
    },
    Pr {
        repo: String,
        number: u64,
        kind: domain::PrKind,
        author: String,
        review_decision: Option<domain::ReviewDecision>,
        head_branch: String,
        base_branch: String,
    },
    #[cfg(feature = "private")]
    MediaBlocked {
        title: String,
        error: String,
    },
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Filter {
    pub(crate) category: Option<Category>,
    pub(crate) query: Option<String>,
}

impl Filter {
    pub(crate) const fn is_empty(&self) -> bool {
        self.category.is_none() && self.query.is_none()
    }
}

pub(crate) struct QueryTerms {
    positives: Vec<String>,
    negatives: Vec<String>,
}

impl QueryTerms {
    pub(crate) fn parse(q: &str) -> Self {
        let mut positives = Vec::new();
        let mut negatives = Vec::new();
        for token in q.split_whitespace() {
            match token.strip_prefix('-') {
                Some(neg) if !neg.is_empty() => negatives.push(neg.to_lowercase()),
                Some(_) => {} // bare '-', ignored
                None => positives.push(token.to_lowercase()),
            }
        }
        Self {
            positives,
            negatives,
        }
    }

    pub(crate) fn matches(&self, lowercased_text: &str) -> bool {
        self.positives
            .iter()
            .all(|p| lowercased_text.contains(p.as_str()))
            && self
                .negatives
                .iter()
                .all(|n| !lowercased_text.contains(n.as_str()))
    }
}
