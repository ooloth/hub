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

#[derive(Clone, Debug)]
pub(crate) struct LineParts {
    pub(crate) primary: String,
    pub(crate) dim_inline: Option<String>,
    pub(crate) dim_suffix: Option<String>,
}

impl LineParts {
    pub(crate) fn flat(&self) -> String {
        let mut s = self.primary.clone();
        if let Some(i) = &self.dim_inline {
            s.push_str(i);
        }
        if let Some(sf) = &self.dim_suffix {
            s.push_str(sf);
        }
        s
    }
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

pub(crate) fn item_tag(item: &StatusItem) -> &'static str {
    match item {
        StatusItem::Pr(_) => "PR",
        StatusItem::Issue(_) => "Issue",
        StatusItem::Ci(_) => "CI",
        StatusItem::Linear(_) => "Linear",
        StatusItem::Loki(_) => "Loki",
        #[cfg(feature = "private")]
        StatusItem::MediaBlocked(_)
        | StatusItem::MediaMissing(_)
        | StatusItem::MediaHealth(_)
        | StatusItem::MediaBacklog { .. } => "Media",
    }
}

pub(crate) fn item_age(item: &StatusItem) -> chrono::Duration {
    match item {
        StatusItem::Pr(pr) => pr.age,
        StatusItem::Issue(i) => i.age,
        StatusItem::Ci(c) => c.age,
        StatusItem::Linear(l) => l.age,
        StatusItem::Loki(l) => l.age,
        #[cfg(feature = "private")]
        StatusItem::MediaBlocked(b) => b.age,
        #[cfg(feature = "private")]
        StatusItem::MediaMissing(m) => m.age,
        #[cfg(feature = "private")]
        StatusItem::MediaHealth(h) => h.age,
        #[cfg(feature = "private")]
        StatusItem::MediaBacklog { .. } => chrono::Duration::zero(),
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
    Loki {
        project: String,
        env: String,
        title: String,
        message: String,
        line: String,
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
        StatusItem::Loki(l) => Some(InvestigationKind::Loki {
            project: l.project.clone(),
            env: l.env.clone(),
            title: l.title.clone(),
            message: l.message.clone(),
            line: l.line.clone(),
        }),
        #[cfg(feature = "private")]
        StatusItem::MediaBlocked(b) => Some(InvestigationKind::MediaBlocked {
            title: b.title.clone(),
            error: b.error.clone(),
        }),
        _ => None,
    }
}

pub(crate) fn item_line(item: &StatusItem) -> LineParts {
    match item {
        StatusItem::Pr(pr) => {
            let badge = match pr.kind {
                domain::PrKind::Mine => "[OPEN]",
                domain::PrKind::ToReview => "[TO REVIEW]",
                domain::PrKind::MyDraft => "[DRAFT]",
            };
            LineParts {
                primary: format!("{} {badge}", pr.title),
                dim_inline: Some(format!(" (#{})", pr.number)),
                dim_suffix: Some(format!(" — {}", pr.repo)),
            }
        }
        StatusItem::Issue(i) => LineParts {
            primary: i.title.clone(),
            dim_inline: Some(format!(" (#{})", i.number)),
            dim_suffix: Some(format!(" — {}", i.repo)),
        },
        StatusItem::Ci(c) => {
            let primary = match (&c.job_name, &c.step_name, &c.error) {
                (Some(job), Some(step), Some(err)) => format!("{job} / {step} · {err}"),
                (Some(job), Some(step), None) => format!("{job} / {step} · failed"),
                (Some(job), None, _) => format!("{job} · failed"),
                _ => "failed".to_string(),
            };
            LineParts {
                primary,
                dim_inline: None,
                dim_suffix: Some(format!(" — {}", c.repo)),
            }
        }
        StatusItem::Linear(l) => LineParts {
            primary: l.title.clone(),
            dim_inline: Some(format!(" ({})", l.identifier)),
            dim_suffix: None,
        },
        StatusItem::Loki(l) => LineParts {
            primary: format!("{} · {}", l.title, l.message),
            dim_inline: None,
            dim_suffix: Some(format!(" — {}:{}", l.project, l.env)),
        },
        #[cfg(feature = "private")]
        StatusItem::MediaBlocked(b) => LineParts {
            primary: format!("Import blocked · {}", b.error),
            dim_inline: None,
            dim_suffix: Some(format!(" — {}", b.source)),
        },
        #[cfg(feature = "private")]
        StatusItem::MediaMissing(m) => LineParts {
            primary: format!("Not found · {} · aired {}", m.title, m.air_date),
            dim_inline: None,
            dim_suffix: Some(format!(" — {}", m.source)),
        },
        #[cfg(feature = "private")]
        StatusItem::MediaHealth(h) => LineParts {
            primary: h.message.clone(),
            dim_inline: None,
            dim_suffix: Some(format!(" — {}", h.source)),
        },
        #[cfg(feature = "private")]
        StatusItem::MediaBacklog { source, count } => LineParts {
            primary: format!("{count} episodes in backlog"),
            dim_inline: None,
            dim_suffix: Some(format!(" — {source}")),
        },
    }
}

pub(crate) fn item_detail_line(item: &StatusItem) -> LineParts {
    #[cfg(feature = "private")]
    if let StatusItem::MediaBlocked(b) = item {
        return LineParts {
            primary: b.title.clone(),
            dim_inline: None,
            dim_suffix: Some(format!(" — {}", b.error)),
        };
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

pub(crate) fn display_item_line(item: &DisplayItem) -> LineParts {
    match item {
        DisplayItem::Single(s) => item_line(s),
        DisplayItem::Group { label, items } => LineParts {
            primary: label.clone(),
            dim_inline: None,
            dim_suffix: Some(format!(" ({})", items.len())),
        },
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
            "{} · {} — {}:{}",
            l.title, l.message, l.project, l.env
        ));
    }
    #[cfg(feature = "private")]
    if let StatusItem::MediaBlocked(b) = item {
        return Some(format!("Import blocked · {} — {}", b.error, b.source));
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
                if !item_line(item)
                    .flat()
                    .to_lowercase()
                    .contains(&q.to_lowercase())
                {
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

    #[test]
    fn snapshot_item_lines() {
        let lines = [
            format!(
                "pr_mine:     {}",
                item_line(&make_pr(domain::PrKind::Mine)).flat()
            ),
            format!(
                "pr_review:   {}",
                item_line(&make_pr(domain::PrKind::ToReview)).flat()
            ),
            format!(
                "pr_draft:    {}",
                item_line(&make_pr(domain::PrKind::MyDraft)).flat()
            ),
            format!("issue:       {}", item_line(&issue()).flat()),
            format!("ci_bare:     {}", item_line(&ci()).flat()),
            format!("ci_job_step: {}", item_line(&ci_job_step()).flat()),
            format!("ci_full:     {}", item_line(&ci_full()).flat()),
            format!("linear:      {}", item_line(&linear()).flat()),
            format!("loki:        {}", item_line(&loki()).flat()),
            format!(
                "group_3:     {}",
                display_item_line(&DisplayItem::Group {
                    label: "MemoryError · OOM killed — project-x:prod".to_string(),
                    items: vec![loki(), loki(), loki()],
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
            format!(
                "media_blocked_detail: {}",
                item_detail_line(&media_blocked()).flat()
            ),
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
    fn item_investigation_pr_returns_none() {
        assert!(item_investigation(&pr()).is_none());
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

    // ── item_tag ─────────────────────────────────────────────────────────────

    #[test]
    fn item_tag_pr_is_pr() {
        assert_eq!(item_tag(&pr()), "PR");
    }

    #[test]
    fn item_tag_issue_is_issue() {
        assert_eq!(item_tag(&issue()), "Issue");
    }

    #[test]
    fn item_tag_ci_is_ci() {
        assert_eq!(item_tag(&ci()), "CI");
    }

    #[test]
    fn item_tag_linear_is_linear() {
        assert_eq!(item_tag(&linear()), "Linear");
    }

    #[test]
    fn item_tag_loki_is_loki() {
        assert_eq!(item_tag(&loki()), "Loki");
    }

    // ── item_age ─────────────────────────────────────────────────────────────

    #[test]
    fn item_age_pr_returns_age_field() {
        let item = StatusItem::Pr(domain::PullRequest {
            age: chrono::Duration::hours(3),
            ..match make_pr(domain::PrKind::Mine) {
                StatusItem::Pr(p) => p,
                _ => unreachable!(),
            }
        });
        assert_eq!(item_age(&item), chrono::Duration::hours(3));
    }

    #[test]
    fn item_age_issue_returns_age_field() {
        let item = StatusItem::Issue(domain::Issue {
            age: chrono::Duration::days(2),
            ..match issue() {
                StatusItem::Issue(i) => i,
                _ => unreachable!(),
            }
        });
        assert_eq!(item_age(&item), chrono::Duration::days(2));
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
