use std::collections::HashMap;
use workflows::status::StatusItem;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub(crate) enum Category {
    Prs,
    Issues,
    Errors,
}

impl Category {
    pub(crate) const ALL: [Category; 3] = [Category::Errors, Category::Prs, Category::Issues];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Category::Prs => "PRs",
            Category::Issues => "Issues",
            Category::Errors => "Errors",
        }
    }
}

#[derive(Debug)]
pub(crate) enum DisplayItem {
    Single(StatusItem),
    Group {
        label: String,
        items: Vec<StatusItem>,
    },
}

#[derive(Debug)]
pub(crate) struct CatData {
    pub(crate) cat: Category,
    pub(crate) items: Vec<DisplayItem>,
}

pub(crate) fn item_url(item: &StatusItem) -> Option<&str> {
    match item {
        StatusItem::Pr(pr) => Some(&pr.url),
        StatusItem::Issue(i) => Some(&i.url),
        StatusItem::Ci(c) => Some(&c.url),
        StatusItem::Linear(l) => Some(&l.url),
        #[cfg(feature = "private")]
        StatusItem::MediaBlocked(_)
        | StatusItem::MediaMissing(_)
        | StatusItem::MediaHealth(_)
        | StatusItem::MediaBacklog { .. } => None,
    }
}

pub(crate) fn item_line(item: &StatusItem) -> String {
    match item {
        StatusItem::Pr(pr) => format!("{} · {} (#{})", pr.repo, pr.title, pr.number),
        StatusItem::Issue(i) => format!("{} · {} (#{})", i.repo, i.title, i.number),
        StatusItem::Ci(c) => format!("{} · {} · {}", c.repo, c.workflow_name, c.conclusion),
        StatusItem::Linear(l) => format!("Linear · {} ({})", l.title, l.identifier),
        #[cfg(feature = "private")]
        StatusItem::MediaBlocked(b) => format!("{} · Import blocked · {}", b.source, b.title),
        #[cfg(feature = "private")]
        StatusItem::MediaMissing(m) => {
            format!(
                "{} · Not found · {} · aired {}",
                m.source, m.title, m.air_date
            )
        }
        #[cfg(feature = "private")]
        StatusItem::MediaHealth(h) => format!("{} · {}", h.source, h.message),
        #[cfg(feature = "private")]
        StatusItem::MediaBacklog { source, count } => {
            format!("{source} · {count} episodes in backlog")
        }
    }
}

pub(crate) fn item_urgency(item: &StatusItem) -> domain::Urgency {
    match item {
        StatusItem::Pr(pr) => pr.urgency,
        StatusItem::Issue(i) => i.urgency,
        StatusItem::Ci(c) => c.urgency,
        StatusItem::Linear(l) => l.urgency,
        #[cfg(feature = "private")]
        StatusItem::MediaBlocked(b) => b.urgency,
        #[cfg(feature = "private")]
        StatusItem::MediaMissing(m) => m.urgency,
        #[cfg(feature = "private")]
        StatusItem::MediaHealth(h) => h.urgency,
        #[cfg(feature = "private")]
        StatusItem::MediaBacklog { .. } => domain::Urgency::Low,
    }
}

pub(crate) fn display_item_line(item: &DisplayItem) -> String {
    match item {
        DisplayItem::Single(s) => item_line(s),
        DisplayItem::Group { label, items } => format!("{} ({})", label, items.len()),
    }
}

pub(crate) fn display_item_urgency(item: &DisplayItem) -> domain::Urgency {
    match item {
        DisplayItem::Single(s) => item_urgency(s),
        DisplayItem::Group { items, .. } => items
            .first()
            .map(item_urgency)
            .unwrap_or(domain::Urgency::Low),
    }
}

pub(crate) fn item_category(item: &StatusItem) -> Category {
    match item {
        StatusItem::Ci(_) => Category::Errors,
        StatusItem::Pr(_) => Category::Prs,
        StatusItem::Issue(_) | StatusItem::Linear(_) => Category::Issues,
        #[cfg(feature = "private")]
        StatusItem::MediaBlocked(_)
        | StatusItem::MediaMissing(_)
        | StatusItem::MediaHealth(_)
        | StatusItem::MediaBacklog { .. } => Category::Errors,
    }
}

pub(crate) fn group_key(_item: &StatusItem) -> Option<String> {
    #[cfg(feature = "private")]
    if let StatusItem::MediaBlocked(b) = _item {
        return Some(format!("{} · Import blocked · {}", b.source, b.error));
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
                .find(|d| matches!(d, DisplayItem::Group { label, .. } if *label == key))
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
            DisplayItem::Group { items, .. } if items.len() == 1 => {
                DisplayItem::Single(items.into_iter().next().unwrap())
            }
            other => other,
        })
        .collect()
}

pub(crate) fn build_cats(items: Vec<StatusItem>) -> Vec<CatData> {
    let mut by_cat: HashMap<Category, Vec<StatusItem>> = HashMap::new();
    for item in items {
        by_cat.entry(item_category(&item)).or_default().push(item);
    }
    Category::ALL
        .iter()
        .map(|&cat| CatData {
            cat,
            items: aggregate(by_cat.remove(&cat).unwrap_or_default()),
        })
        .collect()
}
