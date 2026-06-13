use anyhow::{Context, Result};
use domain::{IssueState, IssueSystem, PrState, RepoSlug, TaskOrigin, TaskStatus};

/// Transitions non-terminal PR-origin tasks whose linked PR has left the open
/// state: merged → `Done`, closed-unmerged → `Failed`.
///
/// Errors are non-fatal from the caller's perspective: `status::run()` appends
/// them to the report's error list and continues — a failed fold-back cycle
/// means tasks stay visible until the next successful refresh.
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

/// Pure decision: given a task's current status and its PR's observed state,
/// return the target `TaskStatus` if a transition should occur, or `None` if
/// the task should stay as-is.
///
/// Guards:
/// - Terminal tasks (`Done`/`Failed`/`Cancelled`) always return `None` — a human
///   correction is never overwritten by signal inference.
/// - `PrState::Open` always returns `None` — the signal hasn't resolved yet.
pub fn decide_fold(status: TaskStatus, pr_state: PrState) -> Option<TaskStatus> {
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
pub fn decide_issue_fold(status: TaskStatus, issue_state: IssueState) -> Option<TaskStatus> {
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
