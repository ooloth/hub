use std::collections::{HashMap, HashSet};

use domain::{AlertSource, IssueSystem, MergeBlocker, SignalIdentity, Task};
use workflows::status::StatusItem;

pub(crate) fn merge_blocker_word(b: MergeBlocker) -> &'static str {
    match b {
        MergeBlocker::Conflict => "conflict",
        MergeBlocker::Behind => "behind",
        MergeBlocker::Blocked => "blocked",
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct GroupKey(String);

impl GroupKey {
    pub(crate) fn new(s: String) -> Self {
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
    pub(crate) fn attached_task(&self) -> Option<&Task> {
        match self {
            FlatRow::BadgedSignal { task, .. } => Some(task),
            _ => None,
        }
    }
}

pub(crate) fn flatten(items: &[DisplayItem], expanded: &HashSet<GroupKey>) -> Vec<FlatRow> {
    let mut rows = Vec::new();
    for item in items {
        match item {
            DisplayItem::Single(s) => rows.push(FlatRow::Single(s.clone())),
            DisplayItem::BadgedSignal { signal, task } => {
                rows.push(FlatRow::BadgedSignal {
                    item: signal.clone(),
                    task: task.clone(),
                });
            }
            DisplayItem::Group {
                label,
                items: group_items,
            } => {
                let is_expanded = expanded.contains(label);
                let urgency = group_items
                    .first()
                    .map(item_urgency)
                    .unwrap_or(domain::Urgency::Low);
                let first_item = group_items
                    .first()
                    .cloned()
                    .unwrap_or_else(|| group_items[0].clone());
                rows.push(FlatRow::GroupHeader {
                    key: label.clone(),
                    count: group_items.len(),
                    urgency,
                    expanded: is_expanded,
                    first_item,
                });
                if is_expanded {
                    let last_idx = group_items.len().saturating_sub(1);
                    for (i, child) in group_items.iter().enumerate() {
                        rows.push(FlatRow::GroupChild {
                            parent_key: label.clone(),
                            item: child.clone(),
                            is_last: i == last_idx,
                        });
                    }
                }
            }
        }
    }
    rows
}

/// A single log line, parsed once at construction.
#[derive(Clone, Debug)]
pub(crate) enum LogLine {
    Json(serde_json::Value),
    Raw(String),
}

impl LogLine {
    pub(crate) fn parse(s: &str) -> Self {
        match serde_json::from_str(s) {
            Ok(v) => LogLine::Json(v),
            Err(_) => LogLine::Raw(s.to_string()),
        }
    }
}

/// View data for the LogDetail screen: shared metadata from one monitoring alert
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

/// Serialise log lines to a compact JSON array string for investigation agents.
/// Json lines are included as-is; Raw lines are wrapped in JSON strings.
pub(crate) fn lines_to_compact_json(lines: &[LogLine]) -> String {
    let arr: Vec<serde_json::Value> = lines
        .iter()
        .map(|l| match l {
            LogLine::Json(v) => v.clone(),
            LogLine::Raw(s) => serde_json::Value::String(s.clone()),
        })
        .collect();
    serde_json::to_string(&serde_json::Value::Array(arr)).unwrap_or_else(|_| "[]".to_string())
}

pub(crate) fn log_detail_view_from_item(item: &StatusItem) -> Option<LogDetailView> {
    match item {
        StatusItem::Gcp(g) => Some(LogDetailView::Gcp {
            project: g.project.clone(),
            env: g.env.clone(),
            title: g.title.clone(),
            message: g.message.clone(),
            lines: vec![LogLine::parse(&g.line)],
        }),
        StatusItem::Loki(l) => Some(LogDetailView::Loki {
            project: l.project.clone(),
            env: l.env.clone(),
            title: l.title.clone(),
            message: l.message.clone(),
            lines: vec![LogLine::parse(&l.line)],
        }),
        _ => None,
    }
}

pub(crate) fn log_detail_view_from_group(items: &[StatusItem]) -> Option<LogDetailView> {
    let first = items
        .iter()
        .find(|i| matches!(i, StatusItem::Gcp(_) | StatusItem::Loki(_)))?;
    match first {
        StatusItem::Gcp(g) => Some(LogDetailView::Gcp {
            project: g.project.clone(),
            env: g.env.clone(),
            title: g.title.clone(),
            message: g.message.clone(),
            lines: items
                .iter()
                .filter_map(|i| match i {
                    StatusItem::Gcp(entry) => Some(LogLine::parse(&entry.line)),
                    _ => None,
                })
                .collect(),
        }),
        StatusItem::Loki(l) => Some(LogDetailView::Loki {
            project: l.project.clone(),
            env: l.env.clone(),
            title: l.title.clone(),
            message: l.message.clone(),
            lines: items
                .iter()
                .filter_map(|i| match i {
                    StatusItem::Loki(entry) => Some(LogLine::parse(&entry.line)),
                    _ => None,
                })
                .collect(),
        }),
        _ => unreachable!(),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub(crate) enum Category {
    Prs,
    Issues,
    Errors,
    Tasks,
}

impl Category {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Category::Prs => "PRs",
            Category::Issues => "Issues",
            Category::Errors => "Errors",
            Category::Tasks => "Tasks",
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
    pub(crate) fn from_item(item: &StatusItem) -> Self {
        match item {
            StatusItem::Pr(_) => SelectedItemKind::Pr,
            StatusItem::Issue(_) => SelectedItemKind::Issue,
            StatusItem::AgentSession(_) => SelectedItemKind::Task,
            _ => SelectedItemKind::Other,
        }
    }

    pub(crate) fn from_row(row: &FlatRow) -> Self {
        match row {
            FlatRow::BadgedSignal { .. } => SelectedItemKind::BadgedSignal,
            FlatRow::Single(item) | FlatRow::GroupChild { item, .. } => Self::from_item(item),
            FlatRow::GroupHeader { .. } => SelectedItemKind::Other,
        }
    }
}

pub(crate) fn item_url(item: &StatusItem) -> Option<&str> {
    match item {
        StatusItem::Pr(pr) => Some(&pr.url),
        StatusItem::Issue(i) => Some(&i.url),
        StatusItem::Ci(c) => Some(&c.url),
        StatusItem::Linear(l) => Some(&l.url),
        StatusItem::Loki(l) => Some(l.url.as_str()).filter(|u| !u.is_empty()),
        StatusItem::Gcp(g) => Some(g.url.as_str()).filter(|u| !u.is_empty()),
        #[cfg(feature = "private")]
        StatusItem::MediaBlocked(b) => Some(&b.url),
        #[cfg(feature = "private")]
        StatusItem::MediaMissing(m) => Some(&m.url),
        #[cfg(feature = "private")]
        StatusItem::MediaHealth(h) => Some(&h.url),
        StatusItem::AgentSession(_) => None,
        #[cfg(feature = "private")]
        StatusItem::MediaBacklog { .. } => None,
    }
}

/// Formats an RFC3339 timestamp as `YYYY-MM-DD HH:MM` for display.
pub(crate) fn fmt_ts(ts: &str) -> String {
    if ts.len() >= 16 {
        ts[..16].replace('T', " ")
    } else {
        ts.to_string()
    }
}

pub(crate) fn format_age_short(d: chrono::Duration) -> String {
    let secs = d.num_seconds();
    if secs < 60 {
        "now".to_string()
    } else if secs < 3600 {
        format!("{}m", d.num_minutes())
    } else if secs < 86400 {
        format!("{}h", d.num_hours())
    } else {
        format!("{}d", d.num_days())
    }
}

pub(crate) fn truncate_to_width(s: &str, w: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= w {
        return s.to_string();
    }
    if w == 0 {
        return String::new();
    }
    chars[..w.saturating_sub(1)].iter().collect::<String>() + "…"
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

pub(crate) fn item_investigation(item: &StatusItem) -> Option<InvestigationKind> {
    match item {
        StatusItem::Ci(c) => Some(InvestigationKind::Ci {
            repo: c.repo.to_string(),
            run_url: c.url.clone(),
        }),
        StatusItem::Issue(i) => Some(InvestigationKind::Issue {
            repo: i.repo.to_string(),
            number: i.number,
        }),
        StatusItem::Pr(pr) => Some(InvestigationKind::Pr {
            repo: pr.repo.to_string(),
            number: pr.number,
            kind: pr.kind,
            author: pr.author.clone(),
            review_decision: pr.review_decision,
            head_branch: pr.head_branch.clone(),
            base_branch: pr.base_branch.clone(),
        }),
        StatusItem::Gcp(g) => Some(InvestigationKind::Gcp {
            project: g.project.clone(),
            env: g.env.clone(),
            title: g.title.clone(),
            message: g.message.clone(),
            line: lines_to_compact_json(&[LogLine::parse(&g.line)]),
            url: g.url.clone(),
            lookback: g.lookback.clone(),
            gcp_project: g.gcp_project.clone(),
        }),
        StatusItem::Loki(l) => Some(InvestigationKind::Loki {
            project: l.project.clone(),
            env: l.env.clone(),
            title: l.title.clone(),
            message: l.message.clone(),
            line: lines_to_compact_json(&[LogLine::parse(&l.line)]),
            url: l.url.clone(),
            lookback: l.lookback.clone(),
        }),
        #[cfg(feature = "private")]
        StatusItem::MediaBlocked(b) => Some(InvestigationKind::MediaBlocked {
            title: b.title.clone(),
            error: b.error.clone(),
        }),
        StatusItem::AgentSession(_) => None,
        _ => None,
    }
}

pub(crate) fn item_line(item: &StatusItem) -> LineParts {
    match item {
        StatusItem::Pr(pr) => {
            let review_status = match pr.review_decision {
                Some(domain::ReviewDecision::ChangesRequested) => "changes requested".to_string(),
                Some(domain::ReviewDecision::Approved) => match pr.approval_count {
                    1 => "1 approval".to_string(),
                    n => format!("{n} approvals"),
                },
                None => "no reviews".to_string(),
            };
            let mut dim_inline = if pr.kind == domain::PrKind::MyDraft {
                vec![
                    format!(" #{}", pr.number),
                    pr.author.clone(),
                    "draft".to_string(),
                ]
            } else {
                vec![format!(" #{}", pr.number), pr.author.clone(), review_status]
            };
            if let Some(blocker) = pr.merge_blocker {
                dim_inline.push(merge_blocker_word(blocker).to_string());
            }
            LineParts {
                separator: RowSeparator::Bullet,
                primary: vec![pr.title.clone()],
                dim_inline,
                source: Some(pr.repo.to_string()),
                category: "PR".to_string(),
                age: format_age_short(pr.age),
            }
        }
        StatusItem::Issue(i) => {
            let dim_inline = if i.labels.is_empty() {
                vec![format!(" #{}", i.number)]
            } else {
                vec![format!(" #{}", i.number), i.labels.join(", ")]
            };
            LineParts {
                separator: RowSeparator::Bullet,
                primary: vec![i.title.clone()],
                dim_inline,
                source: Some(i.repo.to_string()),
                category: "Issue".to_string(),
                age: format_age_short(i.age),
            }
        }
        StatusItem::Ci(c) => {
            let primary = match (&c.job_name, &c.step_name, &c.error) {
                (Some(job), Some(step), Some(err)) => {
                    vec![format!("{job} / {step}"), err.clone()]
                }
                (Some(job), Some(step), None) => {
                    vec![format!("{job} / {step}"), "failed".to_string()]
                }
                (Some(job), None, _) => vec![job.clone(), "failed".to_string()],
                _ => vec!["failed".to_string()],
            };
            LineParts {
                separator: RowSeparator::Bullet,
                primary,
                dim_inline: vec![],
                source: Some(c.repo.to_string()),
                category: "CI".to_string(),
                age: format_age_short(c.age),
            }
        }
        StatusItem::Linear(l) => LineParts {
            separator: RowSeparator::Bullet,
            primary: vec![l.title.clone()],
            dim_inline: vec![format!(" ({})", l.identifier)],
            source: None,
            category: "Linear".to_string(),
            age: format_age_short(l.age),
        },
        StatusItem::Loki(l) => LineParts {
            separator: RowSeparator::Bullet,
            primary: vec![l.title.clone(), l.message.clone()],
            dim_inline: vec![],
            source: Some(format!("{}:{}", l.project, l.env)),
            category: "Loki".to_string(),
            age: format_age_short(l.age),
        },
        StatusItem::Gcp(g) => LineParts {
            separator: RowSeparator::Bullet,
            primary: vec![g.title.clone(), g.message.clone()],
            dim_inline: vec![],
            source: Some(format!("{}:{}", g.project, g.env)),
            category: "GCP".to_string(),
            age: format_age_short(g.age),
        },
        StatusItem::AgentSession(t) => LineParts {
            separator: RowSeparator::Bullet,
            primary: vec![t.id.to_string(), t.title.clone()],
            dim_inline: vec![format!(" {}", t.status), t.kind.to_string()],
            source: None,
            category: "Agent".to_string(),
            age: format_age_short(t.age),
        },
        #[cfg(feature = "private")]
        StatusItem::MediaBlocked(b) => LineParts {
            separator: RowSeparator::Bullet,
            primary: vec!["Import blocked".to_string(), b.error.clone()],
            dim_inline: vec![],
            source: Some(b.source.clone()),
            category: "Media".to_string(),
            age: format_age_short(b.age),
        },
        #[cfg(feature = "private")]
        StatusItem::MediaMissing(m) => LineParts {
            separator: RowSeparator::Bullet,
            primary: vec![
                "Not found".to_string(),
                m.title.clone(),
                format!("aired {}", m.air_date),
            ],
            dim_inline: vec![],
            source: Some(m.source.clone()),
            category: "Media".to_string(),
            age: format_age_short(m.age),
        },
        #[cfg(feature = "private")]
        StatusItem::MediaHealth(h) => LineParts {
            separator: RowSeparator::Bullet,
            primary: vec![h.message.clone()],
            dim_inline: vec![],
            source: Some(h.source.clone()),
            category: "Media".to_string(),
            age: format_age_short(h.age),
        },
        #[cfg(feature = "private")]
        StatusItem::MediaBacklog { source, count } => LineParts {
            separator: RowSeparator::Bullet,
            primary: vec![format!("{count} episodes in backlog")],
            dim_inline: vec![],
            source: Some(source.clone()),
            category: "Media".to_string(),
            age: "now".to_string(),
        },
    }
}

pub(crate) fn item_urgency(item: &StatusItem) -> domain::Urgency {
    match item {
        StatusItem::Pr(pr) => pr.urgency,
        StatusItem::Issue(i) => i.urgency,
        StatusItem::Ci(c) => c.urgency,
        StatusItem::Linear(l) => l.urgency,
        StatusItem::Loki(l) => l.urgency,
        StatusItem::Gcp(g) => g.urgency,
        #[cfg(feature = "private")]
        StatusItem::MediaBlocked(b) => b.urgency,
        #[cfg(feature = "private")]
        StatusItem::MediaMissing(m) => m.urgency,
        #[cfg(feature = "private")]
        StatusItem::MediaHealth(h) => h.urgency,
        StatusItem::AgentSession(t) => t.urgency,
        #[cfg(feature = "private")]
        StatusItem::MediaBacklog { .. } => domain::Urgency::Low,
    }
}

pub(crate) fn flat_row_urgency(row: &FlatRow) -> domain::Urgency {
    match row {
        FlatRow::Single(item) => item_urgency(item),
        FlatRow::GroupHeader { urgency, .. } => *urgency,
        FlatRow::GroupChild { item, .. } => item_urgency(item),
        FlatRow::BadgedSignal { item, .. } => item_urgency(item),
    }
}

pub(crate) fn flat_row_line(row: &FlatRow) -> LineParts {
    match row {
        FlatRow::Single(item) => item_line(item),
        FlatRow::GroupHeader {
            count,
            expanded,
            first_item,
            ..
        } => {
            let base = item_line(first_item);
            LineParts {
                separator: RowSeparator::Toggle(*expanded),
                dim_inline: vec![format!(" ({count})")],
                ..base
            }
        }
        FlatRow::GroupChild { item, is_last, .. } => {
            let mut parts = item_line(item);
            parts.separator = RowSeparator::TreeChild(*is_last);
            parts
        }
        FlatRow::BadgedSignal { item, task } => {
            let mut parts = item_line(item);
            parts
                .dim_inline
                .push(format!("[{} · {}]", task.id, task.status));
            parts
        }
    }
}

pub(crate) fn item_category(item: &StatusItem) -> Category {
    match item {
        StatusItem::Ci(_) | StatusItem::Loki(_) | StatusItem::Gcp(_) => Category::Errors,
        StatusItem::Pr(_) => Category::Prs,
        StatusItem::Issue(_) | StatusItem::Linear(_) => Category::Issues,
        StatusItem::AgentSession(_) => Category::Tasks,
        #[cfg(feature = "private")]
        StatusItem::MediaBlocked(_)
        | StatusItem::MediaMissing(_)
        | StatusItem::MediaHealth(_)
        | StatusItem::MediaBacklog { .. } => Category::Errors,
    }
}

pub(crate) fn group_key(item: &StatusItem) -> Option<GroupKey> {
    if let StatusItem::Gcp(g) = item {
        return Some(GroupKey::new(format!(
            "{} · {} — {}:{}",
            g.title, g.message, g.project, g.env
        )));
    }
    if let StatusItem::Loki(l) = item {
        return Some(GroupKey::new(format!(
            "{} · {} — {}:{}",
            l.title, l.message, l.project, l.env
        )));
    }
    #[cfg(feature = "private")]
    if let StatusItem::MediaBlocked(b) = item {
        return Some(GroupKey::new(format!("Import blocked · {}", b.error)));
    }
    None
}

pub(crate) fn aggregate(items: Vec<StatusItem>) -> Vec<DisplayItem> {
    let mut result: Vec<DisplayItem> = vec![];
    for item in items {
        if let Some(key) = group_key(&item) {
            if let Some(DisplayItem::Group {
                items: group_items, ..
            }) = result
                .iter_mut()
                .find(|d| matches!(d, DisplayItem::Group { label, .. } if label == &key))
            {
                group_items.push(item);
                continue;
            }
            result.push(DisplayItem::Group {
                label: key,
                items: vec![item],
            });
        } else {
            result.push(DisplayItem::Single(item));
        }
    }
    result
        .into_iter()
        .map(|d| match d {
            DisplayItem::Group { label, items } if items.len() == 1 => {
                let _ = label;
                DisplayItem::Single(items.into_iter().next().unwrap())
            }
            other => other,
        })
        .collect()
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Filter {
    pub(crate) category: Option<Category>,
    pub(crate) query: Option<String>,
}

impl Filter {
    pub(crate) fn is_empty(&self) -> bool {
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
        QueryTerms {
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

/// Maps a signal `StatusItem` to its `SignalIdentity` for task-matching.
/// Returns `None` for item types that cannot have tasks attached.
fn signal_identity_for_item(item: &StatusItem) -> Option<SignalIdentity> {
    match item {
        StatusItem::Pr(pr) => Some(SignalIdentity::Pr {
            repo: pr.repo.clone(),
            number: pr.number,
        }),
        StatusItem::Issue(issue) => Some(SignalIdentity::Issue {
            system: IssueSystem::GitHub,
            repo: Some(issue.repo.clone()),
            id: issue.number.to_string(),
        }),
        StatusItem::Linear(linear) => Some(SignalIdentity::Issue {
            system: IssueSystem::Linear,
            repo: None,
            id: linear.identifier.clone(),
        }),
        StatusItem::Ci(ci) => Some(SignalIdentity::Ci {
            repo: ci.repo.clone(),
            workflow: ci.workflow_name.clone(),
            job: ci.job_name.clone(),
            step: ci.step_name.clone(),
        }),
        StatusItem::Loki(loki) => Some(SignalIdentity::Alert {
            source: AlertSource::Loki,
            key: format!("{}/{}/{}", loki.project, loki.env, loki.message),
        }),
        StatusItem::Gcp(gcp) => Some(SignalIdentity::Alert {
            source: AlertSource::Gcp,
            key: format!("{}/{}/{}", gcp.project, gcp.env, gcp.message),
        }),
        StatusItem::AgentSession(_) => None,
        #[cfg(feature = "private")]
        StatusItem::MediaBlocked(_)
        | StatusItem::MediaMissing(_)
        | StatusItem::MediaHealth(_)
        | StatusItem::MediaBacklog { .. } => None,
    }
}

/// Post-aggregate pass: badge signal rows that have an active attached task
/// and suppress the corresponding `AgentSession` row.
///
/// `task_index` is built from the **full** unfiltered item list so that badge
/// matching works even when the active filter hides `AgentSession` rows (e.g.
/// a "PRs" filter — the PR should still carry its badge).
fn badge_and_dedup(
    items: Vec<DisplayItem>,
    task_index: &HashMap<SignalIdentity, Task>,
) -> Vec<DisplayItem> {
    if task_index.is_empty() {
        return items;
    }
    let mut consumed: HashSet<SignalIdentity> = HashSet::new();
    let mut result = Vec::with_capacity(items.len());
    for item in items {
        match &item {
            DisplayItem::Single(signal) if !matches!(signal, StatusItem::AgentSession(_)) => {
                if let Some(identity) = signal_identity_for_item(signal) {
                    if let Some(task) = task_index.get(&identity) {
                        consumed.insert(identity);
                        result.push(DisplayItem::BadgedSignal {
                            signal: signal.clone(),
                            task: task.clone(),
                        });
                        continue;
                    }
                }
            }
            DisplayItem::Single(StatusItem::AgentSession(task))
                if !matches!(task.origin, domain::TaskOrigin::Idea)
                    && consumed.contains(&SignalIdentity::from(&task.origin)) =>
            {
                continue; // suppressed — its signal row already carries the badge
            }
            _ => {}
        }
        result.push(item);
    }
    result
}

pub(crate) fn build_unified(items: Vec<StatusItem>, filter: &Filter) -> Vec<DisplayItem> {
    // Build task index from the full list before filtering, so badge matching
    // works even when the active filter hides AgentSession rows.
    let task_index: HashMap<SignalIdentity, Task> = items
        .iter()
        .filter_map(|item| {
            if let StatusItem::AgentSession(task) = item {
                if !matches!(task.origin, domain::TaskOrigin::Idea) && !task.status.is_terminal() {
                    return Some((SignalIdentity::from(&task.origin), task.clone()));
                }
            }
            None
        })
        .collect();

    let filtered: Vec<StatusItem> = items
        .into_iter()
        .filter(|item| {
            if let Some(cat) = filter.category {
                if item_category(item) != cat {
                    return false;
                }
            }
            if let Some(q) = &filter.query {
                let terms = QueryTerms::parse(q);
                let text = item_line(item).all_text().to_lowercase();
                if !terms.matches(&text) {
                    return false;
                }
            }
            true
        })
        .collect();
    let mut sorted = filtered;
    sorted.sort_by_key(item_urgency);
    badge_and_dedup(aggregate(sorted), &task_index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use workflows::status::StatusItem;

    #[cfg(feature = "private")]
    fn media_blocked() -> StatusItem {
        StatusItem::MediaBlocked(workflows::private::status::BlockedItem {
            source: "Media".to_string(),
            urgency: domain::Urgency::High,
            age: chrono::Duration::zero(),
            title: "Show — S01E01".to_string(),
            error: "Invalid video file".to_string(),
            url: "http://media-server/queue".to_string(),
        })
    }

    #[cfg(feature = "private")]
    fn media_missing() -> StatusItem {
        StatusItem::MediaMissing(workflows::private::status::MissingItem {
            source: "Media".to_string(),
            urgency: domain::Urgency::Medium,
            age: chrono::Duration::zero(),
            title: "Show — S01E02".to_string(),
            air_date: "2024-01-01".to_string(),
            url: "http://media-server/wanted".to_string(),
        })
    }

    #[cfg(feature = "private")]
    fn media_health() -> StatusItem {
        StatusItem::MediaHealth(workflows::private::status::HealthItem {
            source: "Media".to_string(),
            urgency: domain::Urgency::Low,
            age: chrono::Duration::zero(),
            message: "Indexer unavailable".to_string(),
            url: "https://wiki.example.com/system#indexers".to_string(),
        })
    }

    fn pr() -> StatusItem {
        StatusItem::Pr(domain::PullRequest {
            number: 42,
            title: "Add feature".to_string(),
            repo: domain::RepoSlug::new("owner", "repo"),
            url: "https://github.com/owner/repo/pull/42".to_string(),
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::Medium,
            kind: domain::PrKind::ToReview,
            author: "alice".to_string(),
            review_decision: None,
            approval_count: 0,
            comment_count: 0,
            head_branch: "feat/add-feature".to_string(),
            base_branch: "main".to_string(),
            body: None,
            ci_status: None,
            changed_files: vec![],
            total_changed_files: 0,
            review_threads: vec![],
            pr_comments: vec![],
            merge_blocker: None,
        })
    }

    fn issue() -> StatusItem {
        StatusItem::Issue(domain::Issue {
            number: 7,
            title: "Fix bug".to_string(),
            repo: domain::RepoSlug::new("owner", "repo"),
            url: "https://github.com/owner/repo/issues/7".to_string(),
            author: "alice".to_string(),
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::Low,
            labels: vec![],
            body: None,
        })
    }

    fn issue_with_labels() -> StatusItem {
        StatusItem::Issue(domain::Issue {
            number: 8,
            title: "Fix bug".to_string(),
            repo: domain::RepoSlug::new("owner", "repo"),
            url: "https://github.com/owner/repo/issues/8".to_string(),
            author: "alice".to_string(),
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::Low,
            labels: vec!["bug".to_string(), "wontfix".to_string()],
            body: Some("Issue body text.".to_string()),
        })
    }

    fn ci() -> StatusItem {
        StatusItem::Ci(domain::CiFailure {
            repo: domain::RepoSlug::new("owner", "repo"),
            workflow_name: "CI".to_string(),
            job_name: None,
            step_name: None,
            error: None,
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::High,
            url: "https://github.com/owner/repo/actions/runs/1".to_string(),
        })
    }

    fn linear() -> StatusItem {
        StatusItem::Linear(domain::LinearIssue {
            identifier: "ENG-99".to_string(),
            title: "Do the thing".to_string(),
            url: "https://linear.app/eng/issue/ENG-99".to_string(),
            state: "In Progress".to_string(),
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::Medium,
        })
    }

    fn loki() -> StatusItem {
        StatusItem::Loki(domain::LokiEntry {
            title: "MemoryError".to_string(),
            project: "project-x".to_string(),
            env: "prod".to_string(),
            message: "OOM killed".to_string(),
            line: "{}".to_string(),
            lookback: "15m".to_string(),
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::High,
            url: String::new(),
        })
    }

    fn gcp() -> StatusItem {
        StatusItem::Gcp(domain::GcpEntry {
            title: "errors".to_string(),
            project: "mapapp".to_string(),
            env: "neuro".to_string(),
            message: "something broke".to_string(),
            line: "{}".to_string(),
            lookback: "1h".to_string(),
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::High,
            url: "https://console.cloud.google.com/logs/query".to_string(),
            gcp_project: "mapapp-prod-abc123".to_string(),
        })
    }

    fn ci_job_step() -> StatusItem {
        StatusItem::Ci(domain::CiFailure {
            repo: domain::RepoSlug::new("owner", "repo"),
            workflow_name: "CI".to_string(),
            job_name: Some("Build".to_string()),
            step_name: Some("cargo check".to_string()),
            error: None,
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::High,
            url: "https://github.com/owner/repo/actions/runs/1".to_string(),
        })
    }

    fn ci_full() -> StatusItem {
        StatusItem::Ci(domain::CiFailure {
            repo: domain::RepoSlug::new("owner", "repo"),
            workflow_name: "CI".to_string(),
            job_name: Some("Build".to_string()),
            step_name: Some("cargo check".to_string()),
            error: Some("error[E0308]: mismatched types".to_string()),
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::High,
            url: "https://github.com/owner/repo/actions/runs/1".to_string(),
        })
    }

    fn agent_session() -> StatusItem {
        StatusItem::AgentSession(domain::Task {
            id: "TASK-0042".parse().unwrap(),
            title: "Fix auth bug".to_string(),
            description: None,
            status: domain::TaskStatus::InProgress,
            kind: domain::TaskKind::Implement,
            session_id: Some("abc-123".to_string()),
            repo: None,
            origin: domain::TaskOrigin::Idea,
            links: vec![],
            created_at: String::new(),
            updated_at: String::new(),
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::Low,
            comments: vec![],
        })
    }

    fn agent_session_review() -> StatusItem {
        StatusItem::AgentSession(domain::Task {
            id: "TASK-0043".parse().unwrap(),
            title: "Update README".to_string(),
            description: None,
            status: domain::TaskStatus::InReview,
            kind: domain::TaskKind::Implement,
            session_id: None,
            repo: None,
            origin: domain::TaskOrigin::Idea,
            links: vec![],
            created_at: String::new(),
            updated_at: String::new(),
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::High,
            comments: vec![],
        })
    }

    #[test]
    fn snapshot_item_lines() {
        let lines = [
            format!(
                "pr_no_reviews:         {}",
                item_line(&make_pr(domain::PrKind::Mine)).flat()
            ),
            format!(
                "pr_approved:           {}",
                item_line(&make_pr_with(
                    domain::PrKind::Mine,
                    Some(domain::ReviewDecision::Approved),
                    2
                ))
                .flat()
            ),
            format!(
                "pr_changes_requested:  {}",
                item_line(&make_pr_with(
                    domain::PrKind::Mine,
                    Some(domain::ReviewDecision::ChangesRequested),
                    1
                ))
                .flat()
            ),
            format!(
                "pr_draft:              {}",
                item_line(&make_pr(domain::PrKind::MyDraft)).flat()
            ),
            format!(
                "pr_conflict:           {}",
                item_line(&make_pr_conflicting(domain::PrKind::Mine)).flat()
            ),
            format!(
                "pr_draft_conflict:     {}",
                item_line(&make_pr_conflicting(domain::PrKind::MyDraft)).flat()
            ),
            format!("issue:       {}", item_line(&issue()).flat()),
            format!("issue_labels:{}", item_line(&issue_with_labels()).flat()),
            format!("ci_bare:     {}", item_line(&ci()).flat()),
            format!("ci_job_step: {}", item_line(&ci_job_step()).flat()),
            format!("ci_full:     {}", item_line(&ci_full()).flat()),
            format!("linear:      {}", item_line(&linear()).flat()),
            format!("loki:        {}", item_line(&loki()).flat()),
            format!("gcp:         {}", item_line(&gcp()).flat()),
            format!("agent_in_progress: {}", item_line(&agent_session()).flat()),
            format!(
                "agent_review:      {}",
                item_line(&agent_session_review()).flat()
            ),
            format!(
                "group_3:     {}",
                flat_row_line(&FlatRow::GroupHeader {
                    key: GroupKey::new("MemoryError · OOM killed — project-x:prod".to_string()),
                    count: 3,
                    urgency: item_urgency(&loki()),
                    expanded: false,
                    first_item: loki(),
                })
                .flat()
            ),
        ]
        .join("\n");
        insta::assert_snapshot!(lines);
    }

    #[cfg(feature = "private")]
    #[test]
    fn snapshot_item_lines_private() {
        let lines = [
            format!("media_blocked:  {}", item_line(&media_blocked()).flat()),
            format!("media_missing:  {}", item_line(&media_missing()).flat()),
            format!("media_health:   {}", item_line(&media_health()).flat()),
        ]
        .join("\n");
        insta::assert_snapshot!(lines);
    }

    #[test]
    fn item_category_ci_is_errors() {
        assert_eq!(item_category(&ci()), Category::Errors);
    }

    #[test]
    fn item_category_pr_is_prs() {
        assert_eq!(item_category(&pr()), Category::Prs);
    }

    #[test]
    fn item_category_issue_is_issues() {
        assert_eq!(item_category(&issue()), Category::Issues);
    }

    #[test]
    fn item_category_linear_is_issues() {
        assert_eq!(item_category(&linear()), Category::Issues);
    }

    #[test]
    fn item_category_gcp_is_errors() {
        assert_eq!(item_category(&gcp()), Category::Errors);
    }

    #[test]
    fn item_category_agent_session_is_tasks() {
        assert_eq!(item_category(&agent_session()), Category::Tasks);
    }

    #[test]
    fn item_investigation_gcp_returns_kind() {
        assert!(matches!(
            item_investigation(&gcp()),
            Some(InvestigationKind::Gcp { .. })
        ));
    }

    #[test]
    fn item_url_gcp_returns_url() {
        assert_eq!(
            item_url(&gcp()),
            Some("https://console.cloud.google.com/logs/query")
        );
    }

    #[test]
    fn group_key_gcp_returns_key() {
        assert_eq!(
            group_key(&gcp()),
            Some(GroupKey::new(
                "errors · something broke — mapapp:neuro".to_string()
            ))
        );
    }

    fn make_pr(kind: domain::PrKind) -> StatusItem {
        make_pr_with(kind, None, 0)
    }

    fn make_pr_conflicting(kind: domain::PrKind) -> StatusItem {
        let StatusItem::Pr(mut pr) = make_pr_with(kind, None, 0) else {
            unreachable!()
        };
        pr.merge_blocker = Some(domain::MergeBlocker::Conflict);
        StatusItem::Pr(pr)
    }

    fn make_pr_with(
        kind: domain::PrKind,
        review_decision: Option<domain::ReviewDecision>,
        approval_count: u32,
    ) -> StatusItem {
        StatusItem::Pr(domain::PullRequest {
            number: 42,
            title: "Add feature".to_string(),
            repo: domain::RepoSlug::new("owner", "repo"),
            url: "https://github.com/owner/repo/pull/42".to_string(),
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::Medium,
            kind,
            author: "ooloth".to_string(),
            review_decision,
            approval_count,
            comment_count: 0,
            head_branch: "feat/add-feature".to_string(),
            base_branch: "main".to_string(),
            body: None,
            ci_status: None,
            changed_files: vec![],
            total_changed_files: 0,
            review_threads: vec![],
            pr_comments: vec![],
            merge_blocker: None,
        })
    }

    #[test]
    fn item_url_pr_returns_url() {
        assert_eq!(
            item_url(&pr()),
            Some("https://github.com/owner/repo/pull/42")
        );
    }

    #[test]
    fn item_url_ci_returns_url() {
        assert_eq!(
            item_url(&ci()),
            Some("https://github.com/owner/repo/actions/runs/1")
        );
    }

    #[test]
    fn item_url_issue_returns_url() {
        assert_eq!(
            item_url(&issue()),
            Some("https://github.com/owner/repo/issues/7")
        );
    }

    #[cfg(feature = "private")]
    #[test]
    fn item_url_media_blocked_returns_queue_url() {
        assert_eq!(
            item_url(&media_blocked()),
            Some("http://media-server/queue")
        );
    }

    #[cfg(feature = "private")]
    #[test]
    fn item_url_media_missing_returns_wanted_url() {
        assert_eq!(
            item_url(&media_missing()),
            Some("http://media-server/wanted")
        );
    }

    #[cfg(feature = "private")]
    #[test]
    fn item_url_media_health_returns_wiki_url() {
        assert_eq!(
            item_url(&media_health()),
            Some("https://wiki.example.com/system#indexers")
        );
    }

    #[test]
    fn item_investigation_ci_returns_kind() {
        assert!(matches!(
            item_investigation(&ci()),
            Some(InvestigationKind::Ci { .. })
        ));
    }

    #[test]
    fn item_investigation_issue_returns_kind() {
        assert!(matches!(
            item_investigation(&issue()),
            Some(InvestigationKind::Issue { .. })
        ));
    }

    #[test]
    fn item_investigation_pr_returns_pr_kind() {
        assert!(matches!(
            item_investigation(&pr()),
            Some(InvestigationKind::Pr { .. })
        ));
    }

    #[cfg(feature = "private")]
    #[test]
    fn item_investigation_media_blocked_returns_media_kind() {
        assert!(matches!(
            item_investigation(&media_blocked()),
            Some(InvestigationKind::MediaBlocked { .. })
        ));
    }

    #[cfg(feature = "private")]
    #[test]
    fn item_investigation_media_missing_returns_none() {
        assert!(item_investigation(&media_missing()).is_none());
    }

    #[cfg(feature = "private")]
    #[test]
    fn item_investigation_media_health_returns_none() {
        assert!(item_investigation(&media_health()).is_none());
    }

    #[test]
    fn flat_row_urgency_group_header_uses_urgency_field() {
        let header = FlatRow::GroupHeader {
            key: GroupKey::new("g".to_string()),
            count: 2,
            urgency: domain::Urgency::High,
            expanded: false,
            first_item: ci(),
        };
        assert_eq!(flat_row_urgency(&header), domain::Urgency::High);
    }

    #[test]
    fn aggregate_wraps_singles_without_grouping() {
        let items = vec![pr(), issue()];
        let result = aggregate(items);
        assert_eq!(result.len(), 2);
        assert!(matches!(result[0], DisplayItem::Single(_)));
        assert!(matches!(result[1], DisplayItem::Single(_)));
    }

    #[test]
    fn aggregate_empty_input_gives_empty_output() {
        assert!(aggregate(vec![]).is_empty());
    }

    #[test]
    fn build_unified_no_filter_returns_all_items_in_order() {
        let items = vec![ci(), pr(), issue()];
        let result = build_unified(items, &Filter::default());
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn build_unified_empty_input_returns_empty() {
        assert!(build_unified(vec![], &Filter::default()).is_empty());
    }

    #[test]
    fn build_unified_category_filter_keeps_matching_items() {
        let result = build_unified(
            vec![ci(), pr(), issue()],
            &Filter {
                category: Some(Category::Prs),
                query: None,
            },
        );
        assert_eq!(result.len(), 1);
        assert!(matches!(&result[0], DisplayItem::Single(StatusItem::Pr(_))));
    }

    #[test]
    fn build_unified_category_filter_excludes_non_matching() {
        let result = build_unified(
            vec![ci(), pr(), issue()],
            &Filter {
                category: Some(Category::Errors),
                query: None,
            },
        );
        assert_eq!(result.len(), 1);
        assert!(matches!(&result[0], DisplayItem::Single(StatusItem::Ci(_))));
    }

    // issue() fields: primary="Fix bug", dim_inline=" (#7)", source="owner/repo",
    // category="Issue", age=Duration::zero()->"now"
    #[rstest]
    #[case("Fix bug")] // primary
    #[case("fix bug")] // primary, case-insensitive
    #[case("#7")] // dim_inline
    #[case("owner/repo")] // source — currently fails
    #[case("Issue")] // category — currently fails
    #[case("now")] // age — currently fails
    fn build_unified_query_searches_all_visible_text(#[case] query: &str) {
        let result = build_unified(
            vec![issue()],
            &Filter {
                category: None,
                query: Some(query.to_string()),
            },
        );
        assert!(!result.is_empty(), "query {query:?} should match issue");
    }

    #[test]
    fn build_unified_query_filter_matches_case_insensitively() {
        let result = build_unified(
            vec![ci(), pr(), issue()],
            &Filter {
                category: None,
                query: Some("ADD FEATURE".to_string()),
            },
        );
        assert_eq!(result.len(), 1);
        assert!(matches!(&result[0], DisplayItem::Single(StatusItem::Pr(_))));
    }

    #[test]
    fn build_unified_query_filter_no_match_returns_empty() {
        let result = build_unified(
            vec![ci(), pr(), issue()],
            &Filter {
                category: None,
                query: Some("zzznomatch".to_string()),
            },
        );
        assert!(result.is_empty());
    }

    #[test]
    fn build_unified_both_filters_applied_with_and_semantics() {
        let result = build_unified(
            vec![ci(), pr(), issue()],
            &Filter {
                category: Some(Category::Prs),
                query: Some("Add feature".to_string()),
            },
        );
        assert_eq!(result.len(), 1);
        assert!(matches!(&result[0], DisplayItem::Single(StatusItem::Pr(_))));
    }

    #[test]
    fn build_unified_category_and_query_no_overlap_returns_empty() {
        let result = build_unified(
            vec![ci(), pr(), issue()],
            &Filter {
                category: Some(Category::Errors),
                query: Some("Add feature".to_string()), // PR text, not an error
            },
        );
        assert!(result.is_empty());
    }

    // ── format_age_short ─────────────────────────────────────────────────────

    #[test]
    fn format_age_short_zero_is_now() {
        assert_eq!(format_age_short(chrono::Duration::seconds(0)), "now");
    }

    #[test]
    fn format_age_short_59_seconds_is_now() {
        assert_eq!(format_age_short(chrono::Duration::seconds(59)), "now");
    }

    #[test]
    fn format_age_short_60_seconds_is_1m() {
        assert_eq!(format_age_short(chrono::Duration::seconds(60)), "1m");
    }

    #[test]
    fn format_age_short_59_minutes_is_59m() {
        assert_eq!(format_age_short(chrono::Duration::seconds(3599)), "59m");
    }

    #[test]
    fn format_age_short_1_hour_is_1h() {
        assert_eq!(format_age_short(chrono::Duration::seconds(3600)), "1h");
    }

    #[test]
    fn format_age_short_23_hours_is_23h() {
        assert_eq!(format_age_short(chrono::Duration::seconds(86399)), "23h");
    }

    #[test]
    fn format_age_short_1_day_is_1d() {
        assert_eq!(format_age_short(chrono::Duration::seconds(86400)), "1d");
    }

    #[test]
    fn format_age_short_99_days_is_99d() {
        assert_eq!(format_age_short(chrono::Duration::days(99)), "99d");
    }

    // ── truncate_to_width ────────────────────────────────────────────────────

    #[test]
    fn truncate_to_width_fits_exactly_unchanged() {
        assert_eq!(truncate_to_width("hello", 5), "hello");
    }

    #[test]
    fn truncate_to_width_fits_within_unchanged() {
        assert_eq!(truncate_to_width("hello", 10), "hello");
    }

    #[test]
    fn truncate_to_width_overflow_appends_ellipsis() {
        assert_eq!(truncate_to_width("hello", 4), "hel…");
    }

    #[test]
    fn truncate_to_width_width_one_gives_ellipsis() {
        assert_eq!(truncate_to_width("hello", 1), "…");
    }

    #[test]
    fn truncate_to_width_width_zero_gives_empty() {
        assert_eq!(truncate_to_width("hello", 0), "");
    }

    #[test]
    fn truncate_to_width_empty_string_unchanged() {
        assert_eq!(truncate_to_width("", 5), "");
    }

    // ── filter ───────────────────────────────────────────────────────────────

    #[test]
    fn filter_is_empty_when_both_none() {
        assert!(Filter::default().is_empty());
    }

    #[test]
    fn filter_is_not_empty_when_category_set() {
        let f = Filter {
            category: Some(Category::Prs),
            query: None,
        };
        assert!(!f.is_empty());
    }

    #[test]
    fn filter_is_not_empty_when_query_set() {
        let f = Filter {
            category: None,
            query: Some("foo".to_string()),
        };
        assert!(!f.is_empty());
    }

    // ── query parsing: AND logic and negative terms ──────────────────────────

    // pr() all_text: "PR Add feature  #42 alice no reviews owner/repo now"
    // issue() all_text: "Issue Fix bug  #7 owner/repo now"

    #[test]
    fn build_unified_query_and_matches_words_non_adjacently() {
        // "feature" and "alice" both appear in pr() but are not adjacent —
        // old substring match misses this; AND logic finds it
        let result = build_unified(
            vec![pr(), issue()],
            &Filter {
                category: None,
                query: Some("feature alice".to_string()),
            },
        );
        assert_eq!(result.len(), 1);
        assert!(matches!(&result[0], DisplayItem::Single(StatusItem::Pr(_))));
    }

    #[test]
    fn build_unified_query_and_excludes_when_any_word_absent() {
        // pr() has "feature" but not "zzznomatch" — no match
        let result = build_unified(
            vec![pr(), issue()],
            &Filter {
                category: None,
                query: Some("feature zzznomatch".to_string()),
            },
        );
        assert!(result.is_empty());
    }

    #[test]
    fn build_unified_query_negative_term_excludes_matching_items() {
        // "-alice" — "alice" appears in pr()'s dim_inline (author) but not in issue()
        let result = build_unified(
            vec![pr(), issue()],
            &Filter {
                category: None,
                query: Some("-alice".to_string()),
            },
        );
        assert_eq!(result.len(), 1);
        assert!(matches!(
            &result[0],
            DisplayItem::Single(StatusItem::Issue(_))
        ));
    }

    #[test]
    fn build_unified_query_mixed_positive_and_negative() {
        // "owner" appears in both (from "owner/repo"); "-alice" excludes pr()
        let result = build_unified(
            vec![pr(), issue()],
            &Filter {
                category: None,
                query: Some("owner -alice".to_string()),
            },
        );
        assert_eq!(result.len(), 1);
        assert!(matches!(
            &result[0],
            DisplayItem::Single(StatusItem::Issue(_))
        ));
    }

    #[test]
    fn build_unified_query_negative_term_is_case_insensitive() {
        // "-ALICE" excludes pr() — negation terms are lowercased before matching
        let result = build_unified(
            vec![pr(), issue()],
            &Filter {
                category: None,
                query: Some("-ALICE".to_string()),
            },
        );
        assert_eq!(result.len(), 1);
        assert!(matches!(
            &result[0],
            DisplayItem::Single(StatusItem::Issue(_))
        ));
    }

    #[test]
    fn build_unified_query_bare_hyphen_is_ignored() {
        // "- fix": bare "-" is dropped; "fix" matches issue() which has "Fix bug"
        let result = build_unified(
            vec![pr(), issue()],
            &Filter {
                category: None,
                query: Some("- fix".to_string()),
            },
        );
        assert_eq!(result.len(), 1);
        assert!(matches!(
            &result[0],
            DisplayItem::Single(StatusItem::Issue(_))
        ));
    }

    #[test]
    fn build_unified_query_hyphen_mid_word_is_literal_positive() {
        // "fix-bug" — leading char is 'f', not '-', so treated as positive literal
        // issue() has "Fix bug" (space, not hyphen) → no match → empty
        let result = build_unified(
            vec![pr(), issue()],
            &Filter {
                category: None,
                query: Some("fix-bug".to_string()),
            },
        );
        assert!(result.is_empty());
    }

    #[test]
    fn build_unified_query_whitespace_only_matches_all() {
        // "   " — all tokens dropped, effectively no constraint
        let result = build_unified(
            vec![pr(), issue(), ci()],
            &Filter {
                category: None,
                query: Some("   ".to_string()),
            },
        );
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn build_unified_sorts_by_urgency_ascending_critical_first() {
        // issue=Low, pr=Medium, ci=High — input in reverse order
        let result = build_unified(vec![issue(), pr(), ci()], &Filter::default());
        let urgencies: Vec<_> = result
            .iter()
            .map(|item| match item {
                DisplayItem::Single(s) => item_urgency(s),
                DisplayItem::Group { items, .. } => items
                    .first()
                    .map(item_urgency)
                    .unwrap_or(domain::Urgency::Low),
                DisplayItem::BadgedSignal { signal, .. } => item_urgency(signal),
            })
            .collect();
        assert_eq!(
            urgencies,
            vec![
                domain::Urgency::High,
                domain::Urgency::Medium,
                domain::Urgency::Low
            ]
        );
    }

    // --- badge_and_dedup ---

    fn task_for_pr(number: u64, status: domain::TaskStatus) -> domain::Task {
        domain::Task {
            id: "TASK-0099".parse().unwrap(),
            title: "Fix PR".to_string(),
            description: None,
            status,
            kind: domain::TaskKind::Implement,
            session_id: None,
            repo: Some(domain::RepoSlug::new("owner", "repo")),
            origin: domain::TaskOrigin::Pr {
                repo: domain::RepoSlug::new("owner", "repo"),
                number,
            },
            links: vec![],
            created_at: String::new(),
            updated_at: String::new(),
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::Medium,
            comments: vec![],
        }
    }

    fn agent_session_for_pr(number: u64, status: domain::TaskStatus) -> StatusItem {
        StatusItem::AgentSession(task_for_pr(number, status))
    }

    // BD1: A signal with a matching non-terminal task becomes BadgedSignal;
    //      the AgentSession row is suppressed.
    #[test]
    fn badge_and_dedup_badges_signal_and_suppresses_task_row() {
        let task = task_for_pr(42, domain::TaskStatus::InProgress);
        let task_index = HashMap::from([(SignalIdentity::from(&task.origin), task.clone())]);
        let items = vec![
            DisplayItem::Single(pr()),
            DisplayItem::Single(StatusItem::AgentSession(task.clone())),
        ];
        let result = badge_and_dedup(items, &task_index);
        assert_eq!(result.len(), 1, "task row should be suppressed");
        assert!(
            matches!(result[0], DisplayItem::BadgedSignal { .. }),
            "signal row should become BadgedSignal"
        );
    }

    // BD1b: A terminal task does not badge its signal row.
    #[test]
    fn badge_and_dedup_does_not_badge_terminal_task() {
        let items = vec![pr(), agent_session_for_pr(42, domain::TaskStatus::Done)];
        let result = build_unified(items, &Filter::default());
        // Both rows visible; PR is not badged (task row comes after in stable sort).
        assert_eq!(result.len(), 2);
        assert!(matches!(result[0], DisplayItem::Single(StatusItem::Pr(_))));
        assert!(matches!(
            result[1],
            DisplayItem::Single(StatusItem::AgentSession(_))
        ));
    }

    // BD2: A signal with no matching task stays as Single.
    #[test]
    fn badge_and_dedup_leaves_unmatched_signal_as_single() {
        let task = task_for_pr(99, domain::TaskStatus::InProgress);
        let task_index = HashMap::from([(SignalIdentity::from(&task.origin), task)]);
        let items = vec![DisplayItem::Single(pr())]; // pr() uses number 42, not 99
        let result = badge_and_dedup(items, &task_index);
        assert_eq!(result.len(), 1);
        assert!(matches!(result[0], DisplayItem::Single(_)));
    }

    // BD3: An AgentSession row with Idea origin is never suppressed.
    #[test]
    fn badge_and_dedup_keeps_idea_task_row() {
        let task_index = HashMap::new();
        let items = vec![DisplayItem::Single(agent_session())]; // origin = Idea
        let result = badge_and_dedup(items, &task_index);
        assert_eq!(result.len(), 1);
        assert!(matches!(
            result[0],
            DisplayItem::Single(StatusItem::AgentSession(_))
        ));
    }

    // BD4: An AgentSession row with no matching signal stays as Single.
    #[test]
    fn badge_and_dedup_keeps_unmatched_task_row() {
        let task = task_for_pr(42, domain::TaskStatus::InProgress);
        // No matching PR in the item list — task row should survive.
        let task_index = HashMap::from([(SignalIdentity::from(&task.origin), task.clone())]);
        let items = vec![DisplayItem::Single(StatusItem::AgentSession(task))];
        let result = badge_and_dedup(items, &task_index);
        assert_eq!(result.len(), 1);
        assert!(matches!(
            result[0],
            DisplayItem::Single(StatusItem::AgentSession(_))
        ));
    }

    // BD5: build_unified with a PR filter still badges the PR even though
    //      the AgentSession row is filtered out.
    #[test]
    fn build_unified_badges_pr_under_pr_filter() {
        let items = vec![
            pr(),
            agent_session_for_pr(42, domain::TaskStatus::InProgress),
        ];
        let filter = Filter {
            category: Some(Category::Prs),
            query: None,
        };
        let result = build_unified(items, &filter);
        assert_eq!(result.len(), 1, "task row should be absent under PR filter");
        assert!(
            matches!(result[0], DisplayItem::BadgedSignal { .. }),
            "PR should be badged even when task row is filtered out"
        );
    }

    // --- signal_identity_for_item ---

    // SI1: PR maps to Pr identity.
    #[test]
    fn signal_identity_for_pr() {
        assert_eq!(
            signal_identity_for_item(&pr()),
            Some(SignalIdentity::Pr {
                repo: domain::RepoSlug::new("owner", "repo"),
                number: 42,
            })
        );
    }

    // SI2: GitHub issue maps to Issue identity with GitHub system.
    #[test]
    fn signal_identity_for_github_issue() {
        assert_eq!(
            signal_identity_for_item(&issue()),
            Some(SignalIdentity::Issue {
                system: IssueSystem::GitHub,
                repo: Some(domain::RepoSlug::new("owner", "repo")),
                id: "7".to_string(),
            })
        );
    }

    // SI3: Linear issue maps to Issue identity with Linear system.
    #[test]
    fn signal_identity_for_linear_issue() {
        assert_eq!(
            signal_identity_for_item(&linear()),
            Some(SignalIdentity::Issue {
                system: IssueSystem::Linear,
                repo: None,
                id: "ENG-99".to_string(),
            })
        );
    }

    // SI4: CI failure maps to Ci identity (url excluded).
    #[test]
    fn signal_identity_for_ci() {
        assert_eq!(
            signal_identity_for_item(&ci()),
            Some(SignalIdentity::Ci {
                repo: domain::RepoSlug::new("owner", "repo"),
                workflow: "CI".to_string(),
                job: None,
                step: None,
            })
        );
    }

    // SI5: Loki alert maps to Alert identity with project/env/message key.
    #[test]
    fn signal_identity_for_loki() {
        assert_eq!(
            signal_identity_for_item(&loki()),
            Some(SignalIdentity::Alert {
                source: AlertSource::Loki,
                key: "project-x/prod/OOM killed".to_string(),
            })
        );
    }

    // SI6: AgentSession returns None (not a signal).
    #[test]
    fn signal_identity_for_agent_session_is_none() {
        assert_eq!(signal_identity_for_item(&agent_session()), None);
    }

    // --- flat_row_line badge ---

    // FL1: BadgedSignal row includes task id and status in dim_inline.
    #[test]
    fn flat_row_line_badged_signal_appends_badge() {
        let task = task_for_pr(42, domain::TaskStatus::InProgress);
        let row = FlatRow::BadgedSignal {
            item: pr(),
            task: task.clone(),
        };
        let parts = flat_row_line(&row);
        let badge = format!("[{} · {}]", task.id, task.status);
        assert!(
            parts.dim_inline.iter().any(|s| s == &badge),
            "badge should appear in dim_inline"
        );
    }

    // FL2: attached_task returns Some for BadgedSignal, None for Single.
    #[test]
    fn attached_task_returns_task_for_badged_signal() {
        let task = task_for_pr(42, domain::TaskStatus::InProgress);
        let row = FlatRow::BadgedSignal {
            item: pr(),
            task: task.clone(),
        };
        assert!(row.attached_task().is_some());
        assert_eq!(
            row.attached_task().unwrap().id.to_string(),
            task.id.to_string()
        );
        let single = FlatRow::Single(pr());
        assert!(single.attached_task().is_none());
    }

    // SK1: from_row returns BadgedSignal for a BadgedSignal row.
    #[test]
    fn from_row_badged_signal_returns_badged_signal_kind() {
        let task = task_for_pr(42, domain::TaskStatus::InProgress);
        let row = FlatRow::BadgedSignal { item: pr(), task };
        assert_eq!(
            SelectedItemKind::from_row(&row),
            SelectedItemKind::BadgedSignal
        );
    }

    // SK2: from_row delegates to from_item for Single rows.
    #[test]
    fn from_row_single_pr_returns_pr_kind() {
        let row = FlatRow::Single(pr());
        assert_eq!(SelectedItemKind::from_row(&row), SelectedItemKind::Pr);
    }

    // SK3: from_row returns Other for GroupHeader.
    #[test]
    fn from_row_group_header_returns_other() {
        let row = FlatRow::GroupHeader {
            key: GroupKey("g".to_string()),
            count: 1,
            urgency: domain::Urgency::Low,
            expanded: true,
            first_item: pr(),
        };
        assert_eq!(SelectedItemKind::from_row(&row), SelectedItemKind::Other);
    }
}
