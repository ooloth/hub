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

pub(crate) fn item_hint(item: &StatusItem) -> Option<String> {
    match item {
        StatusItem::Ci(_) => Some("↩ to open · i to investigate".to_string()),
        item => item_url(item).map(|_| "↩ to open".to_string()),
    }
}

pub(crate) enum InvestigationKind {
    Ci { repo: String, run_url: String },
}

pub(crate) fn item_investigation(item: &StatusItem) -> Option<InvestigationKind> {
    match item {
        StatusItem::Ci(c) => Some(InvestigationKind::Ci {
            repo: c.repo.to_string(),
            run_url: c.url.clone(),
        }),
        _ => None,
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

#[cfg(test)]
mod tests {
    use super::*;
    use workflows::status::StatusItem;

    fn pr() -> StatusItem {
        StatusItem::Pr(domain::PullRequest {
            number: 42,
            title: "Add feature".to_string(),
            repo: domain::RepoSlug::new("owner", "repo"),
            url: "https://github.com/owner/repo/pull/42".to_string(),
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::Medium,
        })
    }

    fn issue() -> StatusItem {
        StatusItem::Issue(domain::Issue {
            number: 7,
            title: "Fix bug".to_string(),
            repo: domain::RepoSlug::new("owner", "repo"),
            url: "https://github.com/owner/repo/issues/7".to_string(),
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::Low,
            labels: vec![],
        })
    }

    fn ci() -> StatusItem {
        StatusItem::Ci(domain::CiFailure {
            repo: domain::RepoSlug::new("owner", "repo"),
            workflow_name: "CI".to_string(),
            conclusion: "failure".to_string(),
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
    fn item_line_formats_pr() {
        assert_eq!(item_line(&pr()), "owner/repo · Add feature (#42)");
    }

    #[test]
    fn item_line_formats_ci() {
        assert_eq!(item_line(&ci()), "owner/repo · CI · failure");
    }

    #[test]
    fn item_hint_ci_includes_investigate() {
        assert_eq!(
            item_hint(&ci()),
            Some("↩ to open · i to investigate".to_string())
        );
    }

    #[test]
    fn item_hint_pr_is_open_only() {
        assert_eq!(item_hint(&pr()), Some("↩ to open".to_string()));
    }

    #[test]
    fn item_investigation_ci_returns_kind() {
        assert!(matches!(
            item_investigation(&ci()),
            Some(InvestigationKind::Ci { .. })
        ));
    }

    #[test]
    fn item_investigation_pr_returns_none() {
        assert!(item_investigation(&pr()).is_none());
    }

    #[test]
    fn display_item_line_group_shows_label_and_count() {
        let group = DisplayItem::Group {
            label: "Import blocked".to_string(),
            items: vec![ci(), ci()],
        };
        assert_eq!(display_item_line(&group), "Import blocked (2)");
    }

    #[test]
    fn display_item_urgency_group_uses_first_item() {
        let group = DisplayItem::Group {
            label: "g".to_string(),
            items: vec![ci(), issue()],
        };
        assert_eq!(display_item_urgency(&group), domain::Urgency::High);
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
    fn build_cats_produces_categories_in_all_order() {
        let cats = build_cats(vec![]);
        let order: Vec<Category> = cats.iter().map(|c| c.cat).collect();
        assert_eq!(order, Category::ALL.to_vec());
    }

    #[test]
    fn build_cats_routes_items_to_correct_categories() {
        let cats = build_cats(vec![pr(), issue(), ci()]);
        let errors = cats.iter().find(|c| c.cat == Category::Errors).unwrap();
        let prs = cats.iter().find(|c| c.cat == Category::Prs).unwrap();
        let issues = cats.iter().find(|c| c.cat == Category::Issues).unwrap();
        assert_eq!(errors.items.len(), 1);
        assert_eq!(prs.items.len(), 1);
        assert_eq!(issues.items.len(), 1);
    }

    #[test]
    fn snapshot_build_cats_empty() {
        insta::assert_debug_snapshot!(build_cats(vec![]));
    }

    #[test]
    fn snapshot_build_cats_one_of_each_type() {
        insta::assert_debug_snapshot!(build_cats(vec![pr(), issue(), ci(), linear()]));
    }

    #[test]
    fn snapshot_build_cats_multiple_per_category() {
        let pr2 = StatusItem::Pr(domain::PullRequest {
            number: 99,
            title: "Another PR".to_string(),
            repo: domain::RepoSlug::new("owner", "other"),
            url: "https://github.com/owner/other/pull/99".to_string(),
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::High,
        });
        let ci2 = StatusItem::Ci(domain::CiFailure {
            repo: domain::RepoSlug::new("owner", "other"),
            workflow_name: "Lint".to_string(),
            conclusion: "failure".to_string(),
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::Low,
            url: "https://github.com/owner/other/actions/runs/2".to_string(),
        });
        insta::assert_debug_snapshot!(build_cats(vec![pr(), pr2, issue(), ci(), ci2]));
    }

    #[test]
    fn snapshot_build_cats_mixed_urgency() {
        let ci_critical = StatusItem::Ci(domain::CiFailure {
            repo: domain::RepoSlug::new("owner", "repo"),
            workflow_name: "Deploy".to_string(),
            conclusion: "failure".to_string(),
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::Critical,
            url: "https://github.com/owner/repo/actions/runs/3".to_string(),
        });
        let pr_low = StatusItem::Pr(domain::PullRequest {
            number: 1,
            title: "Docs".to_string(),
            repo: domain::RepoSlug::new("owner", "repo"),
            url: "https://github.com/owner/repo/pull/1".to_string(),
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::Low,
        });
        insta::assert_debug_snapshot!(build_cats(vec![ci_critical, pr_low, issue()]));
    }
}
