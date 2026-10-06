//! Asking every source at once, each within a time limit, so one that never answers
//! cannot hold back the rest.
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use anyhow::Result;

use crate::status::StatusItem;

/// What one source came back with.
#[derive(Debug, Default)]
pub(crate) struct SourceAnswer {
    /// The signals it found.
    pub(crate) items: Vec<StatusItem>,
    /// Sources inside it that failed, for a source that runs several (the private workflows).
    pub(crate) failed: Vec<SourceError>,
}

/// A source that failed, with the error it failed with.
///
/// The error is as the source raised it, so it can hold a credential. It is redacted into a
/// [`crate::source_failure::SourceFailure`] before it leaves this crate.
#[derive(Debug)]
pub(crate) struct SourceError {
    /// The source's name, as the report names it.
    pub(crate) source: String,
    /// What it failed with.
    pub(crate) error: anyhow::Error,
}

impl SourceError {
    /// `source` failed with `error`.
    pub(crate) fn new(source: impl Into<String>, error: anyhow::Error) -> Self {
        Self {
            source: source.into(),
            error,
        }
    }
}

/// A source's pending answer.
pub(crate) type Fetch = Pin<Box<dyn Future<Output = Result<SourceAnswer>> + Send>>;

/// One place a refresh asks for signals, named as a failure would be reported.
pub(crate) struct Source {
    /// The name reported when this source fails or runs out of time.
    pub(crate) name: String,
    /// The pending answer.
    pub(crate) fetch: Fetch,
}

impl Source {
    /// A source called `name` whose answer comes from `fetch`.
    pub(crate) fn new(
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

/// Every answer that arrived, and every source that failed or ran out of time.
#[derive(Debug, Default)]
pub(crate) struct Gathered {
    /// Signals from every source that answered, in source order.
    pub(crate) items: Vec<StatusItem>,
    /// Sources that failed or ran out of time, with what they failed with, in source order.
    pub(crate) errors: Vec<SourceError>,
}

/// Asks every source at once and waits at most `limit` for each.
pub(crate) async fn gather(sources: Vec<Source>, limit: Duration) -> Gathered {
    let names: Vec<String> = sources.iter().map(|source| source.name.clone()).collect();
    let answers = futures::future::join_all(
        sources
            .into_iter()
            .map(|source| tokio::time::timeout(limit, source.fetch)),
    )
    .await;

    // Pairing by position would silently drop a source if the two lists ever differed.
    assert_eq!(names.len(), answers.len(), "every source has an answer");
    let mut gathered = Gathered::default();
    for (name, answer) in names.into_iter().zip(answers) {
        match answer {
            Ok(Ok(answer)) => {
                gathered.items.extend(answer.items);
                gathered.errors.extend(answer.failed);
            }
            Ok(Err(error)) => gathered.errors.push(SourceError::new(name, error)),
            Err(_elapsed) => gathered.errors.push(SourceError::new(
                name,
                anyhow::anyhow!("did not answer within {}s", limit.as_secs()),
            )),
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

    /// Each failure as `source: reason`, the reason being the whole error chain.
    fn failures(gathered: &Gathered) -> Vec<String> {
        gathered
            .errors
            .iter()
            .map(|failure| format!("{}: {:#}", failure.source, failure.error))
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
        assert_eq!(failures(&gathered), vec!["hung: did not answer within 60s"]);
        assert_eq!(started.elapsed(), LIMIT);
    }

    #[tokio::test(start_paused = true)]
    async fn a_source_that_errors_is_reported_with_its_whole_error_chain() {
        let sources = vec![
            answering("first", Duration::ZERO, "from first"),
            Source::new("broken", async {
                Err(anyhow::anyhow!("401 Unauthorized").context("failed to reach GitHub API"))
            }),
        ];

        let gathered = gather(sources, LIMIT).await;

        assert_eq!(workflows(&gathered.items), vec!["from first"]);
        assert_eq!(
            failures(&gathered),
            vec!["broken: failed to reach GitHub API: 401 Unauthorized"]
        );
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
    async fn failures_a_source_reports_itself_come_through_with_their_reasons_in_order() {
        let sources = vec![
            Source::new("broken", async { Err(anyhow::anyhow!("down")) }),
            Source::new("private workflows", async {
                Ok(SourceAnswer {
                    items: vec![],
                    failed: vec![SourceError::new(
                        "inner source",
                        anyhow::anyhow!("connection refused"),
                    )],
                })
            }),
        ];

        let gathered = gather(sources, LIMIT).await;

        assert_eq!(
            failures(&gathered),
            vec!["broken: down", "inner source: connection refused"]
        );
    }
}
