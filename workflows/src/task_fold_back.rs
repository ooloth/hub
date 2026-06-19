use anyhow::{Context, Result};
use domain::{
    AlertSource, IssueState, IssueSystem, PrState, RepoSlug, SignalIdentity, TaskOrigin, TaskStatus,
};
use std::collections::HashSet;

use crate::status::{StatusItem, StatusReport};

/// Transitions non-terminal PR-origin tasks whose linked PR has left the open
/// state: merged → `Done`, closed-unmerged → `Failed`.
///
/// Errors are non-fatal from the caller's perspective: `status::run()` appends
/// them to the report's error list and continues — a failed fold-back cycle
/// means tasks stay visible until the next successful refresh.
///
/// # Errors
/// Returns an error if a GitHub or store operation fails.
pub async fn fold_back_pr_tasks(github_token: &str) -> Result<()> {
    let conn = store::status_cache::connect().context("opening status-cache DB")?;
    store::tasks::ensure_table(&conn).context("ensuring tasks table")?;

    let tasks = store::tasks::list_visible(&conn).context("loading visible tasks")?;

    // Collect (repo, number) for every non-terminal task whose origin is a PR.
    // Terminal tasks are excluded here — fold-back never overwrites a human's
    // manual Done/Failed/Cancelled status.
    let pr_tasks: Vec<_> = tasks
        .iter()
        .filter(|t| !t.status.is_terminal())
        .filter_map(|t| match &t.origin {
            TaskOrigin::Pr { repo, number } => Some((t, repo.clone(), *number)),
            _ => None,
        })
        .collect();

    if pr_tasks.is_empty() {
        return Ok(());
    }

    let pairs: Vec<(RepoSlug, u64)> = pr_tasks
        .iter()
        .map(|(_, repo, number)| (repo.clone(), *number))
        .collect();

    let states = clients::github::pr_states(github_token, &pairs)
        .await
        .context("querying PR states for fold-back")?;

    for (task, repo, number) in &pr_tasks {
        let Some(&state) = states.get(&(repo.clone(), *number)) else {
            // Map miss: PR not found or repo unreachable. No action — keep task visible.
            continue;
        };
        if let Some(target) = decide_fold(task.status, state) {
            store::tasks::update_status(&conn, &task.id, target)
                .with_context(|| format!("folding {} to {target:?}", task.id))?;
        }
    }

    Ok(())
}

/// Transitions non-terminal issue-origin tasks whose linked ticket has closed
/// or been resolved: completed → `Done`, cancelled/not-planned → `Cancelled`.
///
/// Handles GitHub Issues and Linear in a single pass. Per-platform query
/// failures are non-fatal from the caller's perspective: `status::run()`
/// appends them to the report's error list and continues.
///
/// # Errors
/// Returns an error if a GitHub, Linear, or store operation fails.
pub async fn fold_back_issue_tasks(github_token: &str, linear_token: Option<&str>) -> Result<()> {
    let conn = store::status_cache::connect().context("opening status-cache DB")?;
    store::tasks::ensure_table(&conn).context("ensuring tasks table")?;

    let tasks = store::tasks::list_visible(&conn).context("loading visible tasks")?;

    // Collect non-terminal issue-origin tasks, partitioned by platform.
    let mut github_tasks: Vec<_> = Vec::new();
    let mut linear_tasks: Vec<_> = Vec::new();
    for task in tasks.iter().filter(|t| !t.status.is_terminal()) {
        match &task.origin {
            TaskOrigin::Issue {
                system: IssueSystem::GitHub,
                repo: Some(repo),
                id,
            } => {
                if let Ok(number) = id.parse::<u64>() {
                    github_tasks.push((task, repo.clone(), number));
                }
            }
            TaskOrigin::Issue {
                system: IssueSystem::Linear,
                id,
                ..
            } => {
                linear_tasks.push((task, id.clone()));
            }
            _ => {}
        }
    }

    // GitHub — batch-query, apply decisions. Non-fatal on error.
    if !github_tasks.is_empty() {
        let pairs: Vec<(RepoSlug, u64)> = github_tasks
            .iter()
            .map(|(_, repo, number)| (repo.clone(), *number))
            .collect();
        match clients::github::issue_states(github_token, &pairs).await {
            Ok(states) => {
                for (task, repo, number) in &github_tasks {
                    let Some(&state) = states.get(&(repo.clone(), *number)) else {
                        continue;
                    };
                    if let Some(target) = decide_issue_fold(task.status, state) {
                        store::tasks::update_status(&conn, &task.id, target)
                            .with_context(|| format!("folding {} to {target:?}", task.id))?;
                    }
                }
            }
            Err(e) => {
                return Err(e.context("querying GitHub issue states for fold-back"));
            }
        }
    }

    // Linear — batch-query, apply decisions. Non-fatal on error.
    if !linear_tasks.is_empty() {
        if let Some(token) = linear_token {
            let identifiers: Vec<String> = linear_tasks.iter().map(|(_, id)| id.clone()).collect();
            match clients::linear::issue_states(token, &identifiers).await {
                Ok(states) => {
                    for (task, id) in &linear_tasks {
                        let Some(&state) = states.get(id.as_str()) else {
                            continue;
                        };
                        if let Some(target) = decide_issue_fold(task.status, state) {
                            store::tasks::update_status(&conn, &task.id, target)
                                .with_context(|| format!("folding {} to {target:?}", task.id))?;
                        }
                    }
                }
                Err(e) => {
                    return Err(e.context("querying Linear issue states for fold-back"));
                }
            }
        }
    }

    Ok(())
}

/// Maps a `StatusItem` to its `SignalIdentity` for CI/alert fold-back.
///
/// Returns `Some` only for `Ci`, `Loki`, and `Gcp` items — the three that
/// produce tasks with matchable origins. All other variants (PR, issue, agent
/// session, media) return `None` and are excluded from the present-set.
fn fold_identity(item: &StatusItem) -> Option<SignalIdentity> {
    match item {
        StatusItem::Ci(ci) => Some(SignalIdentity::Ci {
            repo: ci.repo.clone(),
            workflow: ci.workflow_name.clone(),
            job: ci.job_name.clone(),
            step: ci.step_name.clone(),
        }),
        StatusItem::Loki(l) => Some(SignalIdentity::Alert {
            source: AlertSource::Loki,
            key: format!("{}/{}/{}", l.project, l.env, l.message),
        }),
        StatusItem::Gcp(g) => Some(SignalIdentity::Alert {
            source: AlertSource::Gcp,
            key: format!("{}/{}/{}", g.project, g.env, g.message),
        }),
        _ => None,
    }
}

/// Transitions non-terminal CI/alert-origin tasks whose signal has been absent
/// from two consecutive fetches to `Done`.
///
/// Uses the prior cached `StatusReport` as the second observation point: if the
/// signal was absent from both this refresh's `items` *and* the prior report,
/// the absence is treated as sustained rather than a transient blip. If the
/// prior report is absent or unreadable, the entire pass is skipped so tasks
/// are never folded on insufficient evidence.
///
/// `AlertSource::Media` origins are excluded — tracked separately in #324.
/// Errors are non-fatal from the caller's perspective.
///
/// # Errors
/// Returns an error if the status cache or store cannot be read.
pub fn fold_back_signal_tasks(items: &[StatusItem]) -> Result<()> {
    let conn = store::status_cache::connect().context("opening status-cache DB")?;
    store::tasks::ensure_table(&conn).context("ensuring tasks table")?;

    let present: HashSet<SignalIdentity> = items.iter().filter_map(fold_identity).collect();

    // Read prior report. Absent or unreadable → skip entire pass (conservative).
    let prior_present: HashSet<SignalIdentity> =
        match store::status_cache::read(&conn).context("reading prior status cache")? {
            None => return Ok(()),
            Some(cached) => match serde_json::from_str::<StatusReport>(&cached.payload) {
                Ok(report) => report.items.iter().filter_map(fold_identity).collect(),
                Err(_) => return Ok(()),
            },
        };

    let tasks = store::tasks::list_visible(&conn).context("loading visible tasks")?;

    for task in tasks.iter().filter(|t| !t.status.is_terminal()) {
        let identity = SignalIdentity::from(&task.origin);
        if !matches!(
            &identity,
            SignalIdentity::Ci { .. }
                | SignalIdentity::Alert {
                    source: AlertSource::Loki | AlertSource::Gcp,
                    ..
                }
        ) {
            continue;
        }
        if let Some(target) = decide_signal_fold(
            task.status,
            task.session_id.as_deref(),
            present.contains(&identity),
            prior_present.contains(&identity),
        ) {
            store::tasks::update_status(&conn, &task.id, target)
                .with_context(|| format!("folding {} to {target:?}", task.id))?;
        }
    }

    Ok(())
}

/// Pure decision: given a task's status, session, and whether its signal
/// appears in the current and prior scan, return `Some(Done)` if the task
/// should fold, or `None` if it should stay as-is.
///
/// Guards (all return `None`):
/// - Terminal tasks — never overwritten by fold-back.
/// - Active session (`InProgress`/`Blocked`/`InReview` with `session_id`) — an
///   agent may be the reason the signal cleared; don't fold prematurely.
/// - Signal present in either set — debounce not satisfied.
#[must_use]
pub const fn decide_signal_fold(
    status: TaskStatus,
    session_id: Option<&str>,
    in_present: bool,
    in_prior: bool,
) -> Option<TaskStatus> {
    if status.is_terminal() {
        return None;
    }
    if session_id.is_some()
        && matches!(
            status,
            TaskStatus::InProgress | TaskStatus::Blocked | TaskStatus::InReview
        )
    {
        return None;
    }
    if in_present || in_prior {
        return None;
    }
    Some(TaskStatus::Done)
}

/// Pure decision: given a task's current status and its PR's observed state,
/// return the target `TaskStatus` if a transition should occur, or `None` if
/// the task should stay as-is.
///
/// Guards:
/// - Terminal tasks (`Done`/`Failed`/`Cancelled`) always return `None` — a human
///   correction is never overwritten by signal inference.
/// - `PrState::Open` always returns `None` — the signal hasn't resolved yet.
#[must_use]
pub const fn decide_fold(status: TaskStatus, pr_state: PrState) -> Option<TaskStatus> {
    if status.is_terminal() {
        return None;
    }
    match pr_state {
        PrState::Merged => Some(TaskStatus::Done),
        PrState::Closed => Some(TaskStatus::Failed),
        PrState::Open => None,
    }
}

/// Pure decision: given a task's current status and its issue's observed state,
/// return the target `TaskStatus` if a transition should occur, or `None` if
/// the task should stay as-is.
///
/// Guards:
/// - Terminal tasks (`Done`/`Failed`/`Cancelled`) always return `None` — a human
///   correction is never overwritten by signal inference.
/// - `IssueState::Open` always returns `None` — the signal hasn't resolved yet.
#[must_use]
pub const fn decide_issue_fold(status: TaskStatus, issue_state: IssueState) -> Option<TaskStatus> {
    if status.is_terminal() {
        return None;
    }
    match issue_state {
        IssueState::Completed => Some(TaskStatus::Done),
        IssueState::Cancelled => Some(TaskStatus::Cancelled),
        IssueState::Open => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::{CiFailure, GcpEntry, LokiEntry, RepoSlug, Urgency};

    fn slug() -> RepoSlug {
        RepoSlug::new("ooloth", "hub")
    }

    fn ci_item(job: Option<&str>, step: Option<&str>, url: &str) -> StatusItem {
        StatusItem::Ci(CiFailure {
            repo: slug(),
            workflow_name: "CI".into(),
            job_name: job.map(str::to_string),
            step_name: step.map(str::to_string),
            error: None,
            age: chrono::Duration::zero(),
            urgency: Urgency::High,
            url: url.into(),
        })
    }

    fn loki_item(project: &str, env: &str, message: &str) -> StatusItem {
        StatusItem::Loki(LokiEntry {
            title: "err".into(),
            project: project.into(),
            env: env.into(),
            message: message.into(),
            line: "{}".into(),
            lookback: "15m".into(),
            age: chrono::Duration::zero(),
            urgency: Urgency::Critical,
            url: "https://grafana/x".into(),
        })
    }

    fn gcp_item(project: &str, env: &str, message: &str) -> StatusItem {
        StatusItem::Gcp(GcpEntry {
            title: "err".into(),
            project: project.into(),
            env: env.into(),
            message: message.into(),
            line: "{}".into(),
            lookback: "15m".into(),
            age: chrono::Duration::zero(),
            urgency: Urgency::Critical,
            url: "https://console.cloud.google.com/x".into(),
            gcp_project: "proj-id".into(),
        })
    }

    // ── fold_identity ─────────────────────────────────────────────────────────

    #[test]
    fn ci_items_with_different_urls_produce_the_same_identity() {
        let a = ci_item(Some("build"), Some("test"), "https://github.com/runs/1");
        let b = ci_item(Some("build"), Some("test"), "https://github.com/runs/999");
        assert_eq!(fold_identity(&a), fold_identity(&b));
    }

    #[test]
    fn ci_identity_matches_task_origin_key() {
        let item = ci_item(Some("build"), Some("test"), "https://github.com/runs/1");
        let origin = TaskOrigin::from_ci(match &item {
            StatusItem::Ci(ci) => ci,
            _ => unreachable!(),
        });
        assert_eq!(fold_identity(&item), Some(SignalIdentity::from(&origin)));
    }

    #[test]
    fn loki_identity_matches_task_origin_key() {
        let item = loki_item("myapp", "prod", "db timeout");
        let origin = TaskOrigin::from_loki(match &item {
            StatusItem::Loki(l) => l,
            _ => unreachable!(),
        });
        assert_eq!(fold_identity(&item), Some(SignalIdentity::from(&origin)));
    }

    #[test]
    fn gcp_identity_matches_task_origin_key() {
        let item = gcp_item("myapp", "prod", "oom killed");
        let origin = TaskOrigin::from_gcp(match &item {
            StatusItem::Gcp(g) => g,
            _ => unreachable!(),
        });
        assert_eq!(fold_identity(&item), Some(SignalIdentity::from(&origin)));
    }

    #[test]
    fn loki_items_differing_only_in_title_produce_the_same_identity() {
        let a = loki_item("myapp", "prod", "db timeout");
        let b = loki_item("myapp", "prod", "db timeout");
        assert_eq!(fold_identity(&a), fold_identity(&b));
    }

    #[test]
    fn loki_items_with_different_messages_produce_different_identities() {
        let a = loki_item("myapp", "prod", "db timeout");
        let b = loki_item("myapp", "prod", "oom killed");
        assert_ne!(fold_identity(&a), fold_identity(&b));
    }

    #[rstest::rstest]
    // Terminal tasks are never overwritten
    #[case(TaskStatus::Done, None, false, false, None)]
    #[case(TaskStatus::Failed, None, false, false, None)]
    #[case(TaskStatus::Cancelled, None, false, false, None)]
    // Active session blocks fold for InProgress / Blocked / InReview (D3)
    #[case(TaskStatus::InProgress, Some("s"), false, false, None)]
    #[case(TaskStatus::Blocked, Some("s"), false, false, None)]
    #[case(TaskStatus::InReview, Some("s"), false, false, None)]
    // No session_id → session guard does not apply
    #[case(TaskStatus::InProgress, None, false, false, Some(TaskStatus::Done))]
    #[case(TaskStatus::Blocked, None, false, false, Some(TaskStatus::Done))]
    #[case(TaskStatus::InReview, None, false, false, Some(TaskStatus::Done))]
    // Signal present in fresh scan → debounce not satisfied
    #[case(TaskStatus::Backlog, None, true, false, None)]
    #[case(TaskStatus::Backlog, None, true, true, None)]
    // Signal present in prior only → debounce not satisfied (one-refresh blip)
    #[case(TaskStatus::Backlog, None, false, true, None)]
    // Signal absent from both → fold to Done
    #[case(TaskStatus::Backlog, None, false, false, Some(TaskStatus::Done))]
    #[case(TaskStatus::Ready, None, false, false, Some(TaskStatus::Done))]
    fn decide_signal_fold_cases(
        #[case] status: TaskStatus,
        #[case] session_id: Option<&str>,
        #[case] in_present: bool,
        #[case] in_prior: bool,
        #[case] expected: Option<TaskStatus>,
    ) {
        assert_eq!(
            decide_signal_fold(status, session_id, in_present, in_prior),
            expected
        );
    }

    #[rstest::rstest]
    // Non-terminal + merged → Done
    #[case(TaskStatus::InReview, PrState::Merged, Some(TaskStatus::Done))]
    #[case(TaskStatus::InProgress, PrState::Merged, Some(TaskStatus::Done))]
    #[case(TaskStatus::Backlog, PrState::Merged, Some(TaskStatus::Done))]
    #[case(TaskStatus::Ready, PrState::Merged, Some(TaskStatus::Done))]
    #[case(TaskStatus::Blocked, PrState::Merged, Some(TaskStatus::Done))]
    // Non-terminal + closed-unmerged → Failed
    #[case(TaskStatus::InReview, PrState::Closed, Some(TaskStatus::Failed))]
    #[case(TaskStatus::InProgress, PrState::Closed, Some(TaskStatus::Failed))]
    // Non-terminal + open → no transition
    #[case(TaskStatus::InReview, PrState::Open, None)]
    #[case(TaskStatus::InProgress, PrState::Open, None)]
    // Terminal tasks are never overwritten
    #[case(TaskStatus::Done, PrState::Merged, None)]
    #[case(TaskStatus::Failed, PrState::Closed, None)]
    #[case(TaskStatus::Cancelled, PrState::Merged, None)]
    fn decide_fold_cases(
        #[case] status: TaskStatus,
        #[case] pr_state: PrState,
        #[case] expected: Option<TaskStatus>,
    ) {
        assert_eq!(decide_fold(status, pr_state), expected);
    }

    #[rstest::rstest]
    // Non-terminal + completed → Done
    #[case(TaskStatus::InReview, IssueState::Completed, Some(TaskStatus::Done))]
    #[case(TaskStatus::InProgress, IssueState::Completed, Some(TaskStatus::Done))]
    #[case(TaskStatus::Backlog, IssueState::Completed, Some(TaskStatus::Done))]
    #[case(TaskStatus::Ready, IssueState::Completed, Some(TaskStatus::Done))]
    #[case(TaskStatus::Blocked, IssueState::Completed, Some(TaskStatus::Done))]
    // Non-terminal + cancelled → Cancelled
    #[case(
        TaskStatus::InReview,
        IssueState::Cancelled,
        Some(TaskStatus::Cancelled)
    )]
    #[case(
        TaskStatus::InProgress,
        IssueState::Cancelled,
        Some(TaskStatus::Cancelled)
    )]
    #[case(
        TaskStatus::Backlog,
        IssueState::Cancelled,
        Some(TaskStatus::Cancelled)
    )]
    #[case(TaskStatus::Ready, IssueState::Cancelled, Some(TaskStatus::Cancelled))]
    #[case(
        TaskStatus::Blocked,
        IssueState::Cancelled,
        Some(TaskStatus::Cancelled)
    )]
    // Non-terminal + open → no transition
    #[case(TaskStatus::InReview, IssueState::Open, None)]
    #[case(TaskStatus::InProgress, IssueState::Open, None)]
    // Terminal tasks are never overwritten
    #[case(TaskStatus::Done, IssueState::Completed, None)]
    #[case(TaskStatus::Failed, IssueState::Completed, None)]
    #[case(TaskStatus::Cancelled, IssueState::Completed, None)]
    #[case(TaskStatus::Done, IssueState::Cancelled, None)]
    fn decide_issue_fold_cases(
        #[case] status: TaskStatus,
        #[case] issue_state: IssueState,
        #[case] expected: Option<TaskStatus>,
    ) {
        assert_eq!(decide_issue_fold(status, issue_state), expected);
    }
}
