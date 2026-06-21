use std::collections::HashSet;

use workflows::status::StatusItem;

use super::format::{group_key, item_category, item_line, item_urgency};
use super::types::{DisplayItem, Filter, FlatRow, GroupKey, QueryTerms};

pub(crate) fn flatten(items: &[DisplayItem], expanded: &HashSet<GroupKey>) -> Vec<FlatRow> {
    let mut rows = Vec::new();

    for item in items {
        match item {
            DisplayItem::Single(s) => rows.push(FlatRow::Single(s.clone())),
            DisplayItem::Group {
                label,
                items: group_items,
            } => {
                let is_expanded = expanded.contains(label);

                let urgency = group_items
                    .first()
                    .map_or(domain::Urgency::Low, item_urgency);

                let Some(first_item) = group_items.first().cloned() else {
                    continue;
                };

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
                DisplayItem::Single(
                    items
                        .into_iter()
                        .next()
                        .unwrap_or_else(|| unreachable!("items is non-empty by construction")),
                )
            }
            other => other,
        })
        .collect()
}

pub(crate) fn build_unified(items: Vec<StatusItem>, filter: &Filter) -> Vec<DisplayItem> {
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
    aggregate(sorted)
}
