//! One refresh pass, and the line it leaves in the log.
use std::fmt;

use domain::daemon_pass::{Pass, PassOutcome};
use domain::source_failure::SourceFailure;

use crate::cache::Recording;

/// One pass, as it is logged: what it did, and whether that reached the database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PassReport {
    pub(crate) profile: String,
    /// The pass as it ran.
    pub(crate) pass: Pass,
    pub(crate) recording: Recording,
}

impl PassReport {
    /// Whether this pass replaced the cache, which is what `--once` exits 0 for.
    pub(crate) const fn wrote_the_cache(&self) -> bool {
        self.pass.outcome.payload_written() && matches!(self.recording, Recording::Recorded)
    }
}

impl fmt::Display for PassReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let pass = &self.pass;
        let payload = if self.wrote_the_cache() {
            "written"
        } else {
            "kept"
        };
        write!(
            f,
            "hub-daemon pass at={} pid={} profile={} outcome={} payload={payload} duration_ms={}",
            pass.started_at
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            pass.pid,
            self.profile,
            pass.outcome.status().as_str(),
            pass.duration.as_millis(),
        )?;
        if let Some(items) = pass.outcome.items() {
            write!(f, " items={items}")?;
        }
        match &pass.outcome {
            PassOutcome::Wrote { sources, .. } | PassOutcome::Kept { sources } => {
                write!(
                    f,
                    " answered={} failed_sources={}",
                    sources.answered().len(),
                    sources.failed().len()
                )?;
                write_failed(f, sources.failed())?;
            }
            PassOutcome::Failed { failure } => {
                write!(f, " error=\"{}\"", one_line(failure.reason().as_str()))?;
            }
        }
        match &self.recording {
            Recording::Recorded => Ok(()),
            Recording::RecordedAsFailed { error } => write!(
                f,
                " recorded=as_failed record_error=\"{}\"",
                one_line(error.reason().as_str())
            ),
            Recording::Unrecorded {
                error,
                fallback_error,
            } => {
                write!(
                    f,
                    " recorded=no record_error=\"{}\"",
                    one_line(error.reason().as_str())
                )?;
                fallback_error.as_ref().map_or(Ok(()), |fallback_error| {
                    write!(
                        f,
                        " fallback_error=\"{}\"",
                        one_line(fallback_error.reason().as_str())
                    )
                })
            }
        }
    }
}

/// Appends each failed source with its reason, when there are any.
fn write_failed(f: &mut fmt::Formatter<'_>, failed: &[SourceFailure]) -> fmt::Result {
    if failed.is_empty() {
        return Ok(());
    }
    let failures: Vec<String> = failed
        .iter()
        .map(|failure| format!("{}: {}", failure.source(), failure.reason()))
        .collect();
    write!(f, " failed=\"{}\"", one_line(&failures.join("; ")))
}

/// Text made safe to sit inside one double-quoted value on one line.
fn one_line(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace('"', "'")
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use chrono::{TimeZone, Utc};
    use domain::known_secrets::KnownSecrets;
    use domain::pass_failure::PassFailure;
    use domain::source_outcomes::SourceOutcomes;
    use rstest::rstest;

    use super::*;

    fn no_secrets() -> KnownSecrets {
        KnownSecrets::new(Vec::new())
    }

    fn report(outcome: PassOutcome, recording: Recording) -> PassReport {
        PassReport {
            profile: "dev".to_string(),
            pass: Pass {
                started_at: Utc.with_ymd_and_hms(2026, 10, 3, 23, 15, 55).unwrap(),
                duration: Duration::from_millis(6412),
                pid: 4242,
                outcome,
            },
            recording,
        }
    }

    /// Sources that answered, and sources that failed, each failure as `(name, error)`.
    fn sources(answered: &[&str], failed: &[(&str, &str)]) -> SourceOutcomes {
        SourceOutcomes::new(
            answered.iter().map(|name| (*name).to_string()).collect(),
            failed
                .iter()
                .map(|(source, error)| {
                    SourceFailure::new(
                        *source,
                        &anyhow::anyhow!((*error).to_string()),
                        &no_secrets(),
                    )
                })
                .collect(),
        )
    }

    fn failure(error: &str) -> PassFailure {
        PassFailure::new(&anyhow::anyhow!(error.to_string()), &no_secrets())
    }

    #[rstest]
    #[case::ok(
        PassOutcome::Wrote { items: 1066, sources: sources(&["github issues", "linear issues"], &[]) },
        Recording::Recorded,
        "outcome=ok payload=written duration_ms=6412 items=1066 answered=2 failed_sources=0"
    )]
    #[case::partial_written(
        PassOutcome::Wrote {
            items: 1066,
            sources: sources(
                &["github issues"],
                &[
                    ("private workflows", "connection refused"),
                    ("loki (app · prod)", "did not answer within 60s"),
                ],
            ),
        },
        Recording::Recorded,
        "outcome=partial payload=written duration_ms=6412 items=1066 answered=1 failed_sources=2 failed=\"private workflows: connection refused; loki (app · prod): did not answer within 60s\""
    )]
    #[case::partial_kept(
        PassOutcome::Kept { sources: sources(&["linear issues"], &[("github issues", "401")]) },
        Recording::Recorded,
        "outcome=partial payload=kept duration_ms=6412 answered=1 failed_sources=1 failed=\"github issues: 401\""
    )]
    #[case::no_source_answered(
        PassOutcome::Kept { sources: sources(&[], &[("github issues", "failed to reach GitHub API: dns error")]) },
        Recording::Recorded,
        "outcome=no_source_answered payload=kept duration_ms=6412 answered=0 failed_sources=1 failed=\"github issues: failed to reach GitHub API: dns error\""
    )]
    #[case::pass_failed(
        PassOutcome::Failed { failure: failure("failed to serialize the status report") },
        Recording::Recorded,
        "outcome=pass_failed payload=kept duration_ms=6412 error=\"failed to serialize the status report\""
    )]
    #[case::recorded_as_failed(
        PassOutcome::Wrote { items: 3, sources: sources(&["github issues"], &[]) },
        Recording::RecordedAsFailed { error: failure("payload refused") },
        "outcome=ok payload=kept duration_ms=6412 items=3 answered=1 failed_sources=0 recorded=as_failed record_error=\"payload refused\""
    )]
    #[case::unrecorded(
        PassOutcome::Wrote { items: 3, sources: sources(&["github issues"], &[]) },
        Recording::Unrecorded {
            error: failure("attempt to write a readonly database"),
            fallback_error: Some(failure("attempt to write a readonly database")),
        },
        "outcome=ok payload=kept duration_ms=6412 items=3 answered=1 failed_sources=0 recorded=no record_error=\"attempt to write a readonly database\" fallback_error=\"attempt to write a readonly database\""
    )]
    fn each_pass_is_one_line_saying_what_it_did(
        #[case] outcome: PassOutcome,
        #[case] recording: Recording,
        #[case] tail: &str,
    ) {
        let line = report(outcome, recording).to_string();

        assert_eq!(
            line,
            format!("hub-daemon pass at=2026-10-03T23:15:55Z pid=4242 profile=dev {tail}")
        );
    }

    #[test]
    fn a_line_stays_one_line_whatever_the_error_says() {
        let line = report(
            PassOutcome::Failed {
                failure: failure("first\nsecond \"quoted\""),
            },
            Recording::Recorded,
        )
        .to_string();

        assert!(!line.contains('\n'), "{line}");
        assert!(line.ends_with("error=\"first second 'quoted'\""), "{line}");
    }

    #[rstest]
    #[case::wrote(PassOutcome::Wrote { items: 0, sources: sources(&["a"], &[]) }, Recording::Recorded, true)]
    #[case::wrote_but_recorded_as_failed(
        PassOutcome::Wrote { items: 0, sources: sources(&["a"], &[]) },
        Recording::RecordedAsFailed { error: failure("refused") },
        false
    )]
    #[case::kept(PassOutcome::Kept { sources: sources(&[], &[("a", "down")]) }, Recording::Recorded, false)]
    #[case::failed(PassOutcome::Failed { failure: failure("locked") }, Recording::Recorded, false)]
    fn once_succeeds_only_when_the_cache_was_written(
        #[case] outcome: PassOutcome,
        #[case] recording: Recording,
        #[case] wrote: bool,
    ) {
        assert_eq!(report(outcome, recording).wrote_the_cache(), wrote);
    }
}
