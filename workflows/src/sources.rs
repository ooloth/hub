//! Asking every source at once, each within a time limit, so one that never answers
//! cannot hold back the rest.
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use anyhow::Result;

use crate::status::StatusItem;

/// What one source came back with.
#[derive(Debug, Default)]
pub struct SourceAnswer {
    /// The signals it found.
    pub items: Vec<StatusItem>,
    /// Sources inside it that failed, for a source that runs several (the private workflows).
    pub failed: Vec<String>,
}

/// A source's pending answer.
pub type Fetch = Pin<Box<dyn Future<Output = Result<SourceAnswer>> + Send>>;

/// One place a refresh asks for signals, named as a failure would be reported.
pub struct Source {
    /// The name reported when this source fails or runs out of time.
    pub name: String,
    /// The pending answer.
    pub fetch: Fetch,
}

impl Source {
    /// A source called `name` whose answer comes from `fetch`.
    pub fn new(
        name: impl Into<String>,
        fetch: impl Future<Output = Result<SourceAnswer>> + Send + 'static,
    ) -> Self {
        Self {
            name: name.into(),
            fetch: Box::pin(fetch),
        }
    }
}

impl std::fmt::Debug for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Source")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

/// Every answer that arrived, and every source that failed or ran out of time, by name.
#[derive(Debug, Default)]
pub struct Gathered {
    /// Signals from every source that answered, in source order.
    pub items: Vec<StatusItem>,
    /// Names of sources that failed or ran out of time, in source order.
    pub failed: Vec<String>,
}

/// Asks every source at once and waits at most `limit` for each.
pub async fn gather(sources: Vec<Source>, limit: Duration) -> Gathered {
    let names: Vec<String> = sources.iter().map(|source| source.name.clone()).collect();
    let answers = futures::future::join_all(
        sources
            .into_iter()
            .map(|source| tokio::time::timeout(limit, source.fetch)),
    )
    .await;

    let mut gathered = Gathered::default();
    for (name, answer) in names.into_iter().zip(answers) {
        match answer {
            Ok(Ok(answer)) => {
                gathered.items.extend(answer.items);
                gathered.failed.extend(answer.failed);
            }
            Ok(Err(_)) | Err(_) => gathered.failed.push(name),
        }
    }
    gathered
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMIT: Duration = Duration::from_secs(60);

    fn ci_failure(workflow: &str) -> StatusItem {
        StatusItem::Ci(domain::CiFailure {
            repo: domain::RepoSlug::new("owner", "repo"),
            workflow_name: workflow.to_string(),
            job_name: None,
            step_name: None,
            error: None,
            age: chrono::Duration::zero(),
            urgency: domain::Urgency::High,
            url: "https://github.com/owner/repo/actions/runs/1".to_string(),
        })
    }

    fn workflows(items: &[StatusItem]) -> Vec<String> {
        items
            .iter()
            .map(|item| match item {
                StatusItem::Ci(ci) => ci.workflow_name.clone(),
                _ => panic!("only CI items are used here"),
            })
            .collect()
    }

    fn answering(name: &str, after: Duration, workflow: &str) -> Source {
        let item = ci_failure(workflow);
        Source::new(name, async move {
            tokio::time::sleep(after).await;
            Ok(SourceAnswer {
                items: vec![item],
                failed: vec![],
            })
        })
    }

    #[tokio::test(start_paused = true)]
    async fn a_source_that_never_answers_is_named_and_the_others_are_kept() {
        let started = tokio::time::Instant::now();
        let sources = vec![
            Source::new("hung", std::future::pending()),
            answering("quick", Duration::from_secs(1), "from quick"),
        ];

        let gathered = gather(sources, LIMIT).await;

        assert_eq!(workflows(&gathered.items), vec!["from quick"]);
        assert_eq!(gathered.failed, vec!["hung"]);
        assert_eq!(started.elapsed(), LIMIT);
    }

    #[tokio::test(start_paused = true)]
    async fn a_source_that_errors_is_named_and_the_others_are_kept() {
        let sources = vec![
            answering("first", Duration::ZERO, "from first"),
            Source::new("broken", async { Err(anyhow::anyhow!("401")) }),
        ];

        let gathered = gather(sources, LIMIT).await;

        assert_eq!(workflows(&gathered.items), vec!["from first"]);
        assert_eq!(gathered.failed, vec!["broken"]);
    }

    #[tokio::test(start_paused = true)]
    async fn sources_are_asked_at_the_same_time() {
        let started = tokio::time::Instant::now();
        let sources = vec![
            answering("a", Duration::from_secs(50), "from a"),
            answering("b", Duration::from_secs(50), "from b"),
        ];

        let gathered = gather(sources, LIMIT).await;

        assert_eq!(workflows(&gathered.items), vec!["from a", "from b"]);
        assert_eq!(started.elapsed(), Duration::from_secs(50));
    }

    #[tokio::test(start_paused = true)]
    async fn failures_a_source_reports_itself_come_through_in_order() {
        let sources = vec![
            Source::new("broken", async { Err(anyhow::anyhow!("down")) }),
            Source::new("private workflows", async {
                Ok(SourceAnswer {
                    items: vec![],
                    failed: vec!["inner source".to_string()],
                })
            }),
        ];

        let gathered = gather(sources, LIMIT).await;

        assert_eq!(gathered.failed, vec!["broken", "inner source"]);
    }
}
