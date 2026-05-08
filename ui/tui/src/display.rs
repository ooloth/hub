use workflows::status::StatusItem;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub(crate) enum Category {
    Prs,
    Issues,
    Errors,
}

impl Category {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Category::Prs => "PRs",
            Category::Issues => "Issues",
            Category::Errors => "Errors",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) enum DisplayItem {
    Single(StatusItem),
    Group {
        label: String,
        items: Vec<StatusItem>,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct ListSnapshot {
    pub(crate) items: Vec<DisplayItem>,
    pub(crate) selected: usize,
    pub(crate) filter: Filter,
}

pub(crate) fn item_url(item: &StatusItem) -> Option<&str> {
    match item {
        StatusItem::Pr(pr) => Some(&pr.url),
        StatusItem::Issue(i) => Some(&i.url),
        StatusItem::Ci(c) => Some(&c.url),
        StatusItem::Linear(l) => Some(&l.url),
        StatusItem::Loki(l) => Some(l.url.as_str()).filter(|u| !u.is_empty()),
        #[cfg(feature = "private")]
        StatusItem::MediaBlocked(b) => Some(&b.url),
        #[cfg(feature = "private")]
        StatusItem::MediaMissing(m) => Some(&m.url),
        #[cfg(feature = "private")]
        StatusItem::MediaHealth(h) => Some(&h.url),
        #[cfg(feature = "private")]
        StatusItem::MediaBacklog { .. } => None,
    }
}

pub(crate) fn item_hint(item: &StatusItem) -> Option<String> {
    match item {
        StatusItem::Ci(_) | StatusItem::Loki(_) => Some("↩ to open · i to investigate".to_string()),
        #[cfg(feature = "private")]
        StatusItem::MediaBlocked(_) => Some("↩ to open · i to investigate".to_string()),
        item => item_url(item).map(|_| "↩ to open".to_string()),
    }
}

pub(crate) enum InvestigationKind {
    Ci {
        repo: String,
        run_url: String,
    },
    Loki {
        project: String,
        env: String,
        title: String,
        message: String,
        line: String,
    },
    #[cfg(feature = "private")]
    SonarrBlocked {
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
        StatusItem::Loki(l) => Some(InvestigationKind::Loki {
            project: l.project.clone(),
            env: l.env.clone(),
            title: l.title.clone(),
            message: l.message.clone(),
            line: l.line.clone(),
        }),
        #[cfg(feature = "private")]
        StatusItem::MediaBlocked(b) => Some(InvestigationKind::SonarrBlocked {
            title: b.title.clone(),
            error: b.error.clone(),
        }),
        _ => None,
    }
}

pub(crate) fn item_line(item: &StatusItem) -> String {
    match item {
        StatusItem::Pr(pr) => {
            let badge = match pr.kind {
                domain::PrKind::Mine => "[OPEN]",
                domain::PrKind::ToReview => "[TO REVIEW]",
                domain::PrKind::MyDraft => "[DRAFT]",
            };
            format!("{} · {} (#{}) {badge}", pr.repo, pr.title, pr.number)
        }
        StatusItem::Issue(i) => format!("{} · {} (#{})", i.repo, i.title, i.number),
        StatusItem::Ci(c) => {
            let base = format!("{} · CI", c.repo);
            match (&c.job_name, &c.step_name, &c.error) {
                (Some(job), Some(step), Some(err)) => format!("{base} · {job} / {step} · {err}"),
                (Some(job), Some(step), None) => format!("{base} · {job} / {step} · failed"),
                (Some(job), None, _) => format!("{base} · {job} · failed"),
                _ => format!("{base} · failed"),
            }
        }
        StatusItem::Linear(l) => format!("Linear · {} ({})", l.title, l.identifier),
        StatusItem::Loki(l) => {
            format!("{} · {} · {} · {}", l.project, l.env, l.title, l.message)
        }
        #[cfg(feature = "private")]
        StatusItem::MediaBlocked(b) => format!("{} · Import blocked · {}", b.source, b.error),
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

pub(crate) fn item_detail_line(item: &StatusItem) -> String {
    #[cfg(feature = "private")]
    if let StatusItem::MediaBlocked(b) = item {
        return format!("{} — {}", b.title, b.error);
    }
    item_line(item)
}

pub(crate) fn item_urgency(item: &StatusItem) -> domain::Urgency {
    match item {
        StatusItem::Pr(pr) => pr.urgency,
        StatusItem::Issue(i) => i.urgency,
        StatusItem::Ci(c) => c.urgency,
        StatusItem::Linear(l) => l.urgency,
        StatusItem::Loki(l) => l.urgency,
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
        StatusItem::Ci(_) | StatusItem::Loki(_) => Category::Errors,
        StatusItem::Pr(_) => Category::Prs,
        StatusItem::Issue(_) | StatusItem::Linear(_) => Category::Issues,
        #[cfg(feature = "private")]
        StatusItem::MediaBlocked(_)
        | StatusItem::MediaMissing(_)
        | StatusItem::MediaHealth(_)
        | StatusItem::MediaBacklog { .. } => Category::Errors,
    }
}

pub(crate) fn group_key(item: &StatusItem) -> Option<String> {
    if let StatusItem::Loki(l) = item {
        return Some(format!(
            "{} · {} · {} · {}",
            l.project, l.env, l.title, l.message
        ));
    }
    #[cfg(feature = "private")]
    if let StatusItem::MediaBlocked(b) = item {
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
                if !item_line(item).to_lowercase().contains(&q.to_lowercase()) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use workflows::status::StatusItem;

    #[cfg(feature = "private")]
    fn media_blocked() -> StatusItem {
        StatusItem::MediaBlocked(workflows::private::status::BlockedItem {
            source: "Sonarr".to_string(),
            urgency: domain::Urgency::High,
            age: chrono::Duration::zero(),
            title: "Show — S01E01".to_string(),
            error: "Invalid video file".to_string(),
            url: "http://sonarr/activity/queue".to_string(),
        })
    }

    #[cfg(feature = "private")]
    fn media_missing() -> StatusItem {
        StatusItem::MediaMissing(workflows::private::status::MissingItem {
            source: "Sonarr".to_string(),
            urgency: domain::Urgency::Medium,
            age: chrono::Duration::zero(),
            title: "Show — S01E02".to_string(),
            air_date: "2024-01-01".to_string(),
            url: "http://sonarr/wanted/missing".to_string(),
        })
    }

    #[cfg(feature = "private")]
    fn media_health() -> StatusItem {
        StatusItem::MediaHealth(workflows::private::status::HealthItem {
            source: "Sonarr".to_string(),
            urgency: domain::Urgency::Low,
            age: chrono::Duration::zero(),
            message: "Indexer unavailable".to_string(),
            url: "https://wiki.servarr.com/sonarr/system#indexers".to_string(),
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

    fn make_pr(kind: domain::PrKind) -> StatusItem {
        StatusItem::Pr(domain::PullRequest {
            number: 42,
            title: "Add feature".to_string(),
            repo: domain::RepoSlug::new("owner", "repo"),
            url: "https://github.com/owner/repo/pull/42".to_string(),
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::Medium,
            kind,
        })
    }

    #[test]
    fn item_line_pr_open_badge() {
        assert_eq!(
            item_line(&make_pr(domain::PrKind::Mine)),
            "owner/repo · Add feature (#42) [OPEN]"
        );
    }

    #[test]
    fn item_line_pr_to_review_badge() {
        assert_eq!(
            item_line(&make_pr(domain::PrKind::ToReview)),
            "owner/repo · Add feature (#42) [TO REVIEW]"
        );
    }

    #[test]
    fn item_line_pr_draft_badge() {
        assert_eq!(
            item_line(&make_pr(domain::PrKind::MyDraft)),
            "owner/repo · Add feature (#42) [DRAFT]"
        );
    }

    #[test]
    fn item_line_formats_ci_no_job_info() {
        assert_eq!(item_line(&ci()), "owner/repo · CI · failed");
    }

    #[test]
    fn item_line_formats_ci_with_job_and_step() {
        let item = StatusItem::Ci(domain::CiFailure {
            repo: domain::RepoSlug::new("owner", "repo"),
            workflow_name: "CI".to_string(),
            job_name: Some("Build".to_string()),
            step_name: Some("cargo check".to_string()),
            error: None,
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::High,
            url: "https://github.com/owner/repo/actions/runs/1".to_string(),
        });
        assert_eq!(
            item_line(&item),
            "owner/repo · CI · Build / cargo check · failed"
        );
    }

    #[test]
    fn item_line_formats_ci_with_full_info() {
        let item = StatusItem::Ci(domain::CiFailure {
            repo: domain::RepoSlug::new("owner", "repo"),
            workflow_name: "CI".to_string(),
            job_name: Some("Build".to_string()),
            step_name: Some("cargo check".to_string()),
            error: Some("error[E0308]: mismatched types".to_string()),
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::High,
            url: "https://github.com/owner/repo/actions/runs/1".to_string(),
        });
        assert_eq!(
            item_line(&item),
            "owner/repo · CI · Build / cargo check · error[E0308]: mismatched types"
        );
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
            Some("http://sonarr/activity/queue")
        );
    }

    #[cfg(feature = "private")]
    #[test]
    fn item_url_media_missing_returns_wanted_url() {
        assert_eq!(
            item_url(&media_missing()),
            Some("http://sonarr/wanted/missing")
        );
    }

    #[cfg(feature = "private")]
    #[test]
    fn item_url_media_health_returns_wiki_url() {
        assert_eq!(
            item_url(&media_health()),
            Some("https://wiki.servarr.com/sonarr/system#indexers")
        );
    }

    #[cfg(feature = "private")]
    #[test]
    fn item_hint_media_blocked_includes_investigate() {
        assert_eq!(
            item_hint(&media_blocked()),
            Some("↩ to open · i to investigate".to_string())
        );
    }

    #[cfg(feature = "private")]
    #[test]
    fn item_hint_media_missing_is_open_only() {
        assert_eq!(item_hint(&media_missing()), Some("↩ to open".to_string()));
    }

    #[cfg(feature = "private")]
    #[test]
    fn item_hint_media_health_is_open_only() {
        assert_eq!(item_hint(&media_health()), Some("↩ to open".to_string()));
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

    #[cfg(feature = "private")]
    #[test]
    fn item_investigation_media_blocked_returns_sonarr_kind() {
        assert!(matches!(
            item_investigation(&media_blocked()),
            Some(InvestigationKind::SonarrBlocked { .. })
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

    #[test]
    fn build_unified_sorts_by_urgency_ascending_critical_first() {
        // issue=Low, pr=Medium, ci=High — input in reverse order
        let result = build_unified(vec![issue(), pr(), ci()], &Filter::default());
        let urgencies: Vec<_> = result.iter().map(display_item_urgency).collect();
        assert_eq!(
            urgencies,
            vec![
                domain::Urgency::High,
                domain::Urgency::Medium,
                domain::Urgency::Low
            ]
        );
    }
}
