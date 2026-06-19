use domain::MergeBlocker;
use workflows::status::StatusItem;

use super::types::{
    Category, FlatRow, GroupKey, InvestigationKind, LineParts, LogDetailView, LogLine, RowSeparator,
};

pub(crate) fn merge_blocker_word(b: MergeBlocker) -> &'static str {
    match b {
        MergeBlocker::Conflict => "conflict",
        MergeBlocker::Behind => "behind",
        MergeBlocker::Blocked => "blocked",
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
        _ => None,
    }
}

fn pr_line(pr: &domain::PullRequest) -> LineParts {
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

fn issue_line(i: &domain::Issue) -> LineParts {
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

fn ci_line(c: &domain::CiFailure) -> LineParts {
    let primary = match (&c.job_name, &c.step_name, &c.error) {
        (Some(job), Some(step), Some(err)) => vec![format!("{job} / {step}"), err.clone()],
        (Some(job), Some(step), None) => vec![format!("{job} / {step}"), "failed".to_string()],
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

pub(crate) fn item_line(item: &StatusItem) -> LineParts {
    match item {
        StatusItem::Pr(pr) => pr_line(pr),
        StatusItem::Issue(i) => issue_line(i),
        StatusItem::Ci(c) => ci_line(c),
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
        FlatRow::GroupHeader { urgency, .. } => *urgency,
        FlatRow::Single(item)
        | FlatRow::GroupChild { item, .. }
        | FlatRow::BadgedSignal { item, .. } => item_urgency(item),
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
