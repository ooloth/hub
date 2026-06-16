use std::collections::{HashMap, HashSet};

use domain::{AlertSource, IssueSystem, SignalIdentity, Task};
use workflows::status::StatusItem;

use super::format::{group_key, item_category, item_line, item_urgency};
use super::types::{DisplayItem, Filter, FlatRow, GroupKey, QueryTerms};

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

/// Maps a signal `StatusItem` to its `SignalIdentity` for task-matching.
/// Returns `None` for item types that cannot have tasks attached.
pub(crate) fn signal_identity_for_item(item: &StatusItem) -> Option<SignalIdentity> {
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
pub(crate) fn badge_and_dedup(
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
