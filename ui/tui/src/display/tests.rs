use rstest::rstest;
use workflows::status::StatusItem;

use super::*;
#[cfg(feature = "private")]
use crate::test_support::{media_health, media_missing};

#[cfg(feature = "private")]
fn media_blocked() -> StatusItem {
    StatusItem::MediaBlocked(workflows::private::status::BlockedItem {
        source: "Media".to_string(),
        urgency: domain::Urgency::High,
        age: chrono::Duration::zero(),
        title: domain::UntrustedText::new("Show — S01E01"),
        error: domain::UntrustedText::new("Invalid video file"),
        url: "http://media-server/queue".to_string(),
    })
}

fn pr() -> StatusItem {
    StatusItem::Pr(review_request())
}

/// Someone else's pull request asking for my review, with no reviews yet.
fn review_request() -> domain::PullRequest {
    domain::PullRequest {
        number: 42,
        title: "Add feature".to_string(),
        repo: domain::RepoSlug::new("owner", "repo"),
        url: "https://github.com/owner/repo/pull/42".to_string(),
        urgency: domain::Urgency::Medium,
        kind: domain::PrKind::ToReview,
        author: "alice".to_string(),
        head_branch: "feat/add-feature".to_string(),
        ..crate::test_support::pull_request()
    }
}

/// A pull request of `kind` that I opened, with the reviews given.
fn reviewed(
    kind: domain::PrKind,
    approval_count: u32,
    changes_requested_count: u32,
) -> domain::PullRequest {
    domain::PullRequest {
        kind,
        author: "ooloth".to_string(),
        activity: crate::test_support::verdicts(approval_count, changes_requested_count),
        ..review_request()
    }
}

/// My own draft.
fn my_draft() -> domain::PullRequest {
    domain::PullRequest {
        draft: true,
        ..reviewed(domain::PrKind::Mine, 0, 0)
    }
}

fn issue() -> StatusItem {
    StatusItem::Issue(github_issue())
}

fn github_issue() -> domain::Issue {
    domain::Issue {
        number: 7,
        title: "Fix bug".to_string(),
        repo: domain::RepoSlug::new("owner", "repo"),
        url: "https://github.com/owner/repo/issues/7".to_string(),
        author: "alice".to_string(),
        age: chrono::Duration::zero(),
        urgency: domain::Urgency::Low,
        labels: vec![],
        body: None,
    }
}

fn issue_with_labels() -> StatusItem {
    StatusItem::Issue(domain::Issue {
        number: 8,
        url: "https://github.com/owner/repo/issues/8".to_string(),
        labels: vec!["bug".to_string(), "wontfix".to_string()],
        body: Some("Issue body text.".to_string()),
        ..github_issue()
    })
}

fn ci() -> StatusItem {
    StatusItem::Ci(ci_failure())
}

/// A failed workflow run, with no job, step or error named.
fn ci_failure() -> domain::CiFailure {
    domain::CiFailure {
        repo: domain::RepoSlug::new("owner", "repo"),
        workflow_name: "CI".to_string(),
        job_name: None,
        step_name: None,
        error: None,
        age: chrono::Duration::zero(),
        urgency: domain::Urgency::High,
        url: "https://github.com/owner/repo/actions/runs/1".to_string(),
    }
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
        repo: domain::RepoSlug::new("owner", "project-x"),
        env: "prod".to_string(),
        message: domain::UntrustedText::new("OOM killed"),
        line: domain::UntrustedText::new("{}"),
        lookback: "15m".to_string(),
        age: chrono::Duration::zero(),
        urgency: domain::Urgency::High,
        url: String::new(),
    })
}

fn gcp() -> StatusItem {
    StatusItem::Gcp(domain::GcpEntry {
        title: "errors".to_string(),
        project: "web-app".to_string(),
        repo: domain::RepoSlug::new("owner", "web-app"),
        env: "neuro".to_string(),
        message: domain::UntrustedText::new("something broke"),
        line: domain::UntrustedText::new("{}"),
        lookback: "1h".to_string(),
        age: chrono::Duration::zero(),
        urgency: domain::Urgency::High,
        url: "https://console.cloud.google.com/logs/query".to_string(),
        gcp_project: "web-app-prod-abc123".to_string(),
    })
}

fn ci_job_step() -> StatusItem {
    StatusItem::Ci(domain::CiFailure {
        job_name: Some("Build".to_string()),
        step_name: Some("cargo check".to_string()),
        ..ci_failure()
    })
}

fn ci_full() -> StatusItem {
    StatusItem::Ci(domain::CiFailure {
        job_name: Some("Build".to_string()),
        step_name: Some("cargo check".to_string()),
        error: Some("error[E0308]: mismatched types".to_string()),
        ..ci_failure()
    })
}

fn pr_line(pr: domain::PullRequest) -> String {
    item_line(&StatusItem::Pr(pr)).flat()
}

#[test]
fn snapshot_item_lines() {
    let conflict = domain::MergeReadiness::Blocked(domain::MergeBlocker::Conflict);
    let lines = [
        format!(
            "pr_no_reviews:         {}",
            pr_line(reviewed(domain::PrKind::Mine, 0, 0))
        ),
        format!(
            "pr_approved:           {}",
            pr_line(domain::PullRequest {
                review_decision: Some(domain::ReviewDecision::Approved),
                ..reviewed(domain::PrKind::Mine, 2, 0)
            })
        ),
        format!(
            "pr_changes_requested:  {}",
            pr_line(domain::PullRequest {
                review_decision: Some(domain::ReviewDecision::ChangesRequested),
                ..reviewed(domain::PrKind::Mine, 1, 1)
            })
        ),
        format!(
            "pr_approved_null_decision: {}",
            // Regression: reviewDecision null (e.g. no branch-protection policy)
            // must not hide a counted approval.
            pr_line(reviewed(domain::PrKind::Mine, 1, 0))
        ),
        format!(
            "pr_changes_requested_trumps_approvals: {}",
            // Any changes-requested review shows the bare word, even with approvals.
            pr_line(reviewed(domain::PrKind::Mine, 2, 1))
        ),
        format!("pr_draft:              {}", pr_line(my_draft())),
        format!(
            "pr_conflict:           {}",
            pr_line(domain::PullRequest {
                merge_readiness: conflict,
                ..reviewed(domain::PrKind::Mine, 0, 0)
            })
        ),
        format!(
            "pr_draft_conflict:     {}",
            pr_line(domain::PullRequest {
                merge_readiness: conflict,
                ..my_draft()
            })
        ),
        format!("issue:       {}", item_line(&issue()).flat()),
        format!("issue_labels:{}", item_line(&issue_with_labels()).flat()),
        format!("ci_bare:     {}", item_line(&ci()).flat()),
        format!("ci_job_step: {}", item_line(&ci_job_step()).flat()),
        format!("ci_full:     {}", item_line(&ci_full()).flat()),
        format!("linear:      {}", item_line(&linear()).flat()),
        format!("loki:        {}", item_line(&loki()).flat()),
        format!("gcp:         {}", item_line(&gcp()).flat()),
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

#[rstest]
#[case::ci(ci(), Category::Errors)]
#[case::pr(pr(), Category::Prs)]
#[case::issue(issue(), Category::Issues)]
#[case::linear(linear(), Category::Issues)]
#[case::gcp(gcp(), Category::Errors)]
fn each_signal_is_listed_under_its_category(#[case] item: StatusItem, #[case] category: Category) {
    assert_eq!(item_category(&item), category);
}

#[test]
fn group_key_gcp_returns_key() {
    assert_eq!(
        group_key(&gcp()),
        Some(GroupKey::new(
            "errors · something broke — web-app:neuro".to_string()
        ))
    );
}

#[rstest]
#[case::my_review_is_requested(
    domain::PrKind::ToReview,
    false,
    0,
    0,
    domain::MergeReadiness::NotYetKnown,
    Some("review requested")
)]
#[case::someone_elses_draft_asking_for_my_review(
    domain::PrKind::ToReview,
    true,
    0,
    0,
    domain::MergeReadiness::NotYetKnown,
    None
)]
#[case::mine_approved_and_nothing_blocks_the_merge(
    domain::PrKind::Mine,
    false,
    1,
    0,
    domain::MergeReadiness::Ready,
    Some("ready to merge")
)]
#[case::mine_approved_before_github_decides_the_merge(
    domain::PrKind::Mine,
    false,
    1,
    0,
    domain::MergeReadiness::NotYetKnown,
    None
)]
#[case::mine_approved_with_a_conflict(
    domain::PrKind::Mine,
    false,
    1,
    0,
    domain::MergeReadiness::Blocked(domain::MergeBlocker::Conflict),
    Some("conflicts")
)]
#[case::mine_with_no_reviewer(
    domain::PrKind::Mine,
    false,
    0,
    0,
    domain::MergeReadiness::Ready,
    Some("no reviewer")
)]
#[case::mine_with_changes_requested(
    domain::PrKind::Mine,
    false,
    1,
    1,
    domain::MergeReadiness::Ready,
    Some("feedback")
)]
#[case::my_draft(
    domain::PrKind::Mine,
    true,
    1,
    0,
    domain::MergeReadiness::Ready,
    Some("draft")
)]
#[case::someone_elses_not_awaiting_my_review(
    domain::PrKind::External,
    false,
    1,
    0,
    domain::MergeReadiness::Ready,
    None
)]
fn a_pr_row_names_why_it_is_my_turn_exactly_when_it_is(
    #[case] kind: domain::PrKind,
    #[case] draft: bool,
    #[case] approvals: u32,
    #[case] changes_requested: u32,
    #[case] merge: domain::MergeReadiness,
    #[case] expected: Option<&str>,
) {
    let line = item_line(&StatusItem::Pr(domain::PullRequest {
        draft,
        merge_readiness: merge,
        ..reviewed(kind, approvals, changes_requested)
    }));

    assert_eq!(line.subcategory.as_deref(), expected);
}

#[test]
fn searching_for_a_reason_finds_only_the_rows_that_show_it() {
    let ready = StatusItem::Pr(domain::PullRequest {
        merge_readiness: domain::MergeReadiness::Ready,
        ..reviewed(domain::PrKind::Mine, 1, 0)
    });
    let waiting = StatusItem::Pr(domain::PullRequest {
        merge_readiness: domain::MergeReadiness::Ready,
        ..reviewed(domain::PrKind::Mine, 0, 0)
    });

    let result = build_unified(
        vec![ready, waiting],
        &Filter {
            category: None,
            query: Some("ready to merge".to_string()),
        },
    );

    assert_eq!(result.len(), 1);
    assert!(matches!(
        &result[0],
        DisplayItem::Single(StatusItem::Pr(pr))
            if pr.activity.count(domain::pr_activity::Opinion::Approved) == 1
    ));
}

#[rstest]
#[case::pr(pr(), "https://github.com/owner/repo/pull/42")]
#[case::ci(ci(), "https://github.com/owner/repo/actions/runs/1")]
#[case::issue(issue(), "https://github.com/owner/repo/issues/7")]
#[case::gcp(gcp(), "https://console.cloud.google.com/logs/query")]
fn each_signal_opens_its_own_url(#[case] item: StatusItem, #[case] url: &str) {
    assert_eq!(item_url(&item), Some(url));
}

#[cfg(feature = "private")]
#[rstest]
#[case::media_blocked_opens_the_queue(media_blocked(), "http://media-server/queue")]
#[case::media_missing_opens_wanted(media_missing(), "http://media-server/wanted")]
#[case::media_health_opens_the_wiki(media_health(), "https://wiki.example.com/system#indexers")]
fn each_media_signal_opens_its_own_url(#[case] item: StatusItem, #[case] url: &str) {
    assert_eq!(item_url(&item), Some(url));
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
fn signals_without_a_group_stay_separate_items() {
    let items = vec![pr(), issue()];
    let result = aggregate(items);
    assert_eq!(result.len(), 2);
    assert!(matches!(result[0], DisplayItem::Single(_)));
    assert!(matches!(result[1], DisplayItem::Single(_)));
}

#[test]
fn no_signals_make_no_display_items() {
    assert!(aggregate(vec![]).is_empty());
}

#[test]
fn no_signals_make_no_rows() {
    assert!(build_unified(vec![], &Filter::default()).is_empty());
}

// issue() fields: primary="Fix bug", dim_inline=" (#7)", source="owner/repo",
// category="Issue", age=Duration::zero()->"now"
#[rstest]
#[case("Fix bug")] // primary
#[case("fix bug")] // primary, case-insensitive
#[case("#7")] // dim_inline
#[case("owner/repo")] // source
#[case("Issue")] // category
#[case("now")] // age
fn a_query_searches_all_of_a_rows_visible_text(#[case] query: &str) {
    let result = build_unified(
        vec![issue()],
        &Filter {
            category: None,
            query: Some(query.to_string()),
        },
    );
    assert!(!result.is_empty(), "query {query:?} should match issue");
}

// ── filter ───────────────────────────────────────────────────────────────

/// The category of each row a filter on `category` and `query` keeps from `ci()`, `pr()` and
/// `issue()`.
///
/// The CI failure is High, the pull request Medium and the issue Low, so the rows come back in
/// that order. `pr()` searches as "PR Add feature  #42 alice no reviews owner/repo now" and
/// `issue()` as "Issue Fix bug  #7 owner/repo now".
fn rows_kept(category: Option<Category>, query: Option<&str>) -> Vec<Category> {
    let filter = Filter {
        category,
        query: query.map(str::to_string),
    };
    build_unified(vec![ci(), pr(), issue()], &filter)
        .iter()
        .map(|row| match row {
            DisplayItem::Single(item) => item_category(item),
            DisplayItem::Group { .. } => panic!("none of these signals groups"),
        })
        .collect()
}

#[rstest]
#[case::no_filter(None, None, vec![Category::Errors, Category::Prs, Category::Issues])]
#[case::category_keeps_its_own(Some(Category::Prs), None, vec![Category::Prs])]
#[case::category_drops_the_rest(Some(Category::Errors), None, vec![Category::Errors])]
#[case::query_ignores_case(None, Some("ADD FEATURE"), vec![Category::Prs])]
#[case::query_matching_nothing(None, Some("zzznomatch"), vec![])]
#[case::category_and_query_both_apply(Some(Category::Prs), Some("Add feature"), vec![Category::Prs])]
#[case::category_and_query_that_never_overlap(Some(Category::Errors), Some("Add feature"), vec![])]
#[case::words_need_not_be_adjacent(None, Some("feature alice"), vec![Category::Prs])]
#[case::every_word_must_match(None, Some("feature zzznomatch"), vec![])]
#[case::a_negative_word_drops_its_matches(None, Some("-alice"), vec![Category::Errors, Category::Issues])]
#[case::positive_and_negative_words_combine(None, Some("owner -alice"), vec![Category::Errors, Category::Issues])]
#[case::a_negative_word_ignores_case(None, Some("-ALICE"), vec![Category::Errors, Category::Issues])]
#[case::a_bare_hyphen_is_ignored(None, Some("- fix"), vec![Category::Issues])]
#[case::a_hyphen_inside_a_word_is_literal(None, Some("fix-bug"), vec![])]
#[case::a_blank_query_keeps_everything(None, Some("   "), vec![Category::Errors, Category::Prs, Category::Issues])]
fn a_filter_keeps_only_the_rows_it_matches(
    #[case] category: Option<Category>,
    #[case] query: Option<&str>,
    #[case] expected: Vec<Category>,
) {
    assert_eq!(rows_kept(category, query), expected);
}

#[rstest]
#[case::nothing_set(Filter::default(), true)]
#[case::a_category(Filter { category: Some(Category::Prs), query: None }, false)]
#[case::a_query(Filter { category: None, query: Some("foo".to_string()) }, false)]
fn a_filter_is_empty_only_when_it_sets_nothing(#[case] filter: Filter, #[case] empty: bool) {
    assert_eq!(filter.is_empty(), empty);
}

// ── format_age_short ─────────────────────────────────────────────────────

#[rstest]
#[case::zero(0, "now")]
#[case::under_a_minute(59, "now")]
#[case::one_minute(60, "1m")]
#[case::under_an_hour(3599, "59m")]
#[case::one_hour(3600, "1h")]
#[case::under_a_day(86399, "23h")]
#[case::one_day(86400, "1d")]
#[case::ninety_nine_days(99 * 86400, "99d")]
fn an_age_is_shown_in_its_largest_whole_unit(#[case] seconds: i64, #[case] shown: &str) {
    assert_eq!(format_age_short(chrono::Duration::seconds(seconds)), shown);
}

// ── truncate_to_width ────────────────────────────────────────────────────

#[rstest]
#[case::fits_exactly("hello", 5, "hello")]
#[case::fits_within("hello", 10, "hello")]
#[case::overflows("hello", 4, "hel…")]
#[case::width_one("hello", 1, "…")]
#[case::width_zero("hello", 0, "")]
#[case::empty_text("", 5, "")]
fn text_wider_than_its_width_is_cut_with_an_ellipsis(
    #[case] text: &str,
    #[case] width: usize,
    #[case] shown: &str,
) {
    assert_eq!(truncate_to_width(text, width), shown);
}

#[test]
fn the_most_urgent_signal_is_listed_first() {
    // issue=Low, pr=Medium, ci=High — input in reverse order
    let result = build_unified(vec![issue(), pr(), ci()], &Filter::default());
    let urgencies: Vec<_> = result
        .iter()
        .map(|item| match item {
            DisplayItem::Single(s) => item_urgency(s),
            DisplayItem::Group { items, .. } => match items.first() {
                Some(first) => item_urgency(first),
                None => domain::Urgency::Low,
            },
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

#[test]
fn a_selected_pr_row_offers_pr_actions() {
    let row = FlatRow::Single(pr());
    assert_eq!(SelectedItemKind::from_row(&row), SelectedItemKind::Pr);
}

#[test]
fn a_selected_group_header_offers_no_item_actions() {
    let row = FlatRow::GroupHeader {
        key: GroupKey::new("g".to_string()),
        count: 1,
        urgency: domain::Urgency::Low,
        expanded: true,
        first_item: pr(),
    };
    assert_eq!(SelectedItemKind::from_row(&row), SelectedItemKind::Other);
}

/// The owner takes room on every row and says little once the repo is named,
/// so the row shows only the repo.
#[rstest]
#[case::pr(pr())]
#[case::issue(issue())]
#[case::ci(ci())]
fn a_repo_row_shows_only_the_repo_name(#[case] item: StatusItem) {
    assert_eq!(item_line(&item).source.as_deref(), Some("repo"));
}

/// Filtering by owner is what the owner is still for once it is hidden.
#[rstest]
#[case::pr(pr())]
#[case::issue(issue())]
#[case::ci(ci())]
fn a_repo_row_can_still_be_found_by_its_owner(#[case] item: StatusItem) {
    let line = item_line(&item);

    assert_eq!(line.search_only, vec!["owner/repo".to_string()]);
    assert!(
        line.all_text().contains("owner/repo"),
        "{}",
        line.all_text()
    );
}

#[rstest]
#[case("owner", 1)]
#[case("owner/repo", 1)]
#[case("-owner", 0)]
fn searching_by_owner_finds_rows_that_no_longer_show_it(
    #[case] query: &str,
    #[case] expected: usize,
) {
    let result = build_unified(
        vec![pr()],
        &Filter {
            category: None,
            query: Some(query.to_string()),
        },
    );

    assert_eq!(result.len(), expected);
}

/// Alert rows name their project and environment, which carry no owner.
#[rstest]
#[case::loki(loki())]
#[case::gcp(gcp())]
fn an_alert_row_still_shows_its_project_and_environment(#[case] item: StatusItem) {
    let line = item_line(&item);

    assert!(
        line.source.as_deref().is_some_and(|s| s.contains(':')),
        "{:?}",
        line.source
    );
    assert_eq!(line.search_only, Vec::<String>::new());
}
