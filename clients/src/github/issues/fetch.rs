use anyhow::{Context, Result};
use domain::{Issue, RepoSlug, Urgency, NEEDS_HUMAN_REVIEW_LABEL};
use serde::Deserialize;

use super::super::age;

// ── REST API types ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ApiIssue {
    number: u64,
    title: String,
    html_url: String,
    user: ApiUser,
    created_at: String,
    #[serde(default)]
    labels: Vec<ApiLabel>,
    #[serde(default)]
    assignees: Vec<ApiAssignee>,
    #[serde(default)]
    pull_request: Option<PullRequestMarker>,
    body: Option<String>,
}

#[derive(Deserialize)]
struct ApiUser {
    login: String,
}

#[derive(Deserialize)]
pub(super) struct ApiLabel {
    pub(super) name: String,
}

#[derive(Deserialize)]
struct ApiAssignee {
    login: String,
}

#[derive(Deserialize)]
struct PullRequestMarker {}

// ── Public fetch function ─────────────────────────────────────────────────────

/// Returns all open issues across the given repos. Issues assigned to `username`
/// or labelled `status:needs-human-review` receive `Urgency::Medium`; all others
/// receive `Urgency::Low`. Pull requests returned by the API are filtered out.
///
/// Fetches all pages per repo (up to 100 per page) in parallel across repos.
///
/// # Errors
/// Returns an error if any GitHub API call fails.
pub async fn issues(token: &str, repos: &[String], username: &str) -> Result<Vec<Issue>> {
    if repos.is_empty() {
        return Ok(vec![]);
    }

    let futures: Vec<_> = repos
        .iter()
        .map(|repo| {
            let token = token.to_string();
            let repo = repo.clone();
            async move {
                let (owner, name) = repo
                    .split_once('/')
                    .ok_or_else(|| anyhow::anyhow!("invalid repo slug: {repo}"))?;
                let raw = fetch_repo_issues(&token, owner, name).await?;
                Ok::<(RepoSlug, Vec<ApiIssue>), anyhow::Error>((RepoSlug::new(owner, name), raw))
            }
        })
        .collect();

    let results = futures::future::join_all(futures).await;
    let mut issues = Vec::new();
    for result in results {
        let (repo_slug, raw_items) = result?;
        for item in raw_items {
            if let Some(issue) = to_domain_issue(item, repo_slug.clone(), username) {
                issues.push(issue);
            }
        }
    }
    Ok(issues)
}

// ── Internal helpers ──────────────────────────────────────────────────────────

async fn fetch_repo_issues(token: &str, owner: &str, repo: &str) -> Result<Vec<ApiIssue>> {
    let mut all = Vec::new();
    let mut page = 1u32;
    loop {
        let page_s = page.to_string();
        let items: Vec<ApiIssue> = reqwest::Client::new()
            .get(format!(
                "https://api.github.com/repos/{owner}/{repo}/issues"
            ))
            .query(&[
                ("state", "open"),
                ("per_page", "100"),
                ("page", page_s.as_str()),
            ])
            .bearer_auth(token)
            .header("User-Agent", "hub-cli")
            .header("Accept", "application/vnd.github.v3+json")
            .send()
            .await
            .with_context(|| format!("failed to reach GitHub API for {owner}/{repo}"))?
            .error_for_status()
            .with_context(|| format!("GitHub API error for {owner}/{repo}"))?
            .json()
            .await
            .with_context(|| {
                format!("failed to parse GitHub issues response for {owner}/{repo}")
            })?;
        let done = !has_next_page(items.len(), 100);
        all.extend(items);
        if done {
            break;
        }
        page += 1;
    }
    Ok(all)
}

fn to_domain_issue(item: ApiIssue, repo: RepoSlug, username: &str) -> Option<Issue> {
    if item.pull_request.is_some() {
        return None;
    }
    let assignees: Vec<String> = item.assignees.into_iter().map(|a| a.login).collect();
    let labels: Vec<String> = item.labels.into_iter().map(|l| l.name).collect();
    Some(Issue {
        number: item.number,
        title: item.title,
        repo,
        url: item.html_url,
        author: item.user.login,
        age: age(&item.created_at),
        urgency: classify_urgency(&assignees, &labels, username),
        labels,
        body: item.body,
    })
}

fn classify_urgency(assignees: &[String], labels: &[String], username: &str) -> Urgency {
    if assignees.iter().any(|a| a == username)
        || labels.iter().any(|l| l == NEEDS_HUMAN_REVIEW_LABEL)
    {
        Urgency::Medium
    } else {
        Urgency::Low
    }
}

fn has_next_page(returned: usize, per_page: usize) -> bool {
    returned == per_page
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── classify_urgency ──────────────────────────────────────────────────────

    #[rstest::rstest]
    #[case(vec!["alice"], vec![], "alice", Urgency::Medium)]
    #[case(vec![], vec!["status:needs-human-review"], "alice", Urgency::Medium)]
    #[case(vec!["alice"], vec!["status:needs-human-review"], "alice", Urgency::Medium)]
    #[case(vec![], vec![], "alice", Urgency::Low)]
    #[case(vec!["bob"], vec![], "alice", Urgency::Low)]
    #[case(vec!["bob"], vec!["other-label"], "alice", Urgency::Low)]
    fn classify_urgency_cases(
        #[case] assignees: Vec<&str>,
        #[case] labels: Vec<&str>,
        #[case] username: &str,
        #[case] expected: Urgency,
    ) {
        let assignees: Vec<String> = assignees.into_iter().map(str::to_string).collect();
        let labels: Vec<String> = labels.into_iter().map(str::to_string).collect();
        assert_eq!(classify_urgency(&assignees, &labels, username), expected);
    }

    // ── has_next_page ─────────────────────────────────────────────────────────

    #[test]
    fn has_next_page_true_when_full_page_returned() {
        assert!(has_next_page(100, 100));
    }

    #[test]
    fn has_next_page_false_when_short_page_returned() {
        assert!(!has_next_page(99, 100));
    }

    #[test]
    fn has_next_page_false_when_empty_page_returned() {
        assert!(!has_next_page(0, 100));
    }

    // ── to_domain_issue ───────────────────────────────────────────────────────

    fn make_api_issue(pull_request: Option<PullRequestMarker>) -> ApiIssue {
        ApiIssue {
            number: 7,
            title: "Fix the thing".to_string(),
            html_url: "https://github.com/owner/repo/issues/7".to_string(),
            user: ApiUser {
                login: "bob".to_string(),
            },
            created_at: "2024-01-01T00:00:00Z".to_string(),
            labels: vec![ApiLabel {
                name: "bug".to_string(),
            }],
            assignees: vec![ApiAssignee {
                login: "alice".to_string(),
            }],
            pull_request,
            body: Some("This is the issue body.".to_string()),
        }
    }

    #[test]
    fn to_domain_issue_returns_none_for_pull_request() {
        let item = make_api_issue(Some(PullRequestMarker {}));
        assert!(to_domain_issue(item, RepoSlug::new("owner", "repo"), "alice").is_none());
    }

    #[test]
    fn to_domain_issue_maps_fields_correctly() {
        let item = make_api_issue(None);
        let issue = to_domain_issue(item, RepoSlug::new("owner", "repo"), "alice").unwrap();
        assert_eq!(issue.number, 7);
        assert_eq!(issue.title, "Fix the thing");
        assert_eq!(issue.repo.to_string(), "owner/repo");
        assert_eq!(issue.url, "https://github.com/owner/repo/issues/7");
        assert_eq!(issue.author, "bob");
        assert_eq!(issue.labels, vec!["bug"]);
        assert_eq!(issue.urgency, Urgency::Medium); // assigned to alice
        assert_eq!(issue.body, Some("This is the issue body.".to_string()));
    }

    #[test]
    fn to_domain_issue_body_none_when_absent() {
        let mut item = make_api_issue(None);
        item.body = None;
        let issue = to_domain_issue(item, RepoSlug::new("owner", "repo"), "alice").unwrap();
        assert_eq!(issue.body, None);
    }

    #[test]
    fn to_domain_issue_unassigned_no_label_is_low_urgency() {
        let mut item = make_api_issue(None);
        item.assignees = vec![];
        item.labels = vec![];
        let issue = to_domain_issue(item, RepoSlug::new("owner", "repo"), "alice").unwrap();
        assert_eq!(issue.urgency, Urgency::Low);
    }
}
