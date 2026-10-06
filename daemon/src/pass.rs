//! One refresh pass, and the line it leaves in the log.
use std::fmt;
use std::time::Duration;

use chrono::{DateTime, Utc};
use workflows::source_failure::SourceFailure;

/// What a pass did with the cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PassOutcome {
    /// The cache was replaced. `failed` holds the sources that failed while others answered.
    Wrote {
        items: usize,
        failed: Vec<SourceFailure>,
    },
    /// Every source failed, so the cache kept what it had.
    Unchanged { failed: Vec<SourceFailure> },
    /// The pass itself failed, for example writing the cache.
    Failed { error: String },
}

/// One pass, as it is logged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PassReport {
    pub(crate) at: DateTime<Utc>,
    pub(crate) pid: u32,
    pub(crate) profile: String,
    pub(crate) duration: Duration,
    pub(crate) outcome: PassOutcome,
}

impl PassReport {
    /// Whether this pass replaced the cache, which is what `--once` exits 0 for.
    pub(crate) const fn wrote_the_cache(&self) -> bool {
        matches!(self.outcome, PassOutcome::Wrote { .. })
    }
}

impl fmt::Display for PassReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "hub-daemon pass at={} pid={} profile={} ",
            self.at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            self.pid,
            self.profile,
        )?;
        let duration_ms = self.duration.as_millis();
        match &self.outcome {
            PassOutcome::Wrote { items, failed } => {
                let outcome = if failed.is_empty() { "ok" } else { "partial" };
                write!(
                    f,
                    "outcome={outcome} duration_ms={duration_ms} items={items} failed_sources={}",
                    failed.len()
                )?;
                write_failed(f, failed)
            }
            PassOutcome::Unchanged { failed } => {
                write!(
                    f,
                    "outcome=unchanged duration_ms={duration_ms} failed_sources={}",
                    failed.len()
                )?;
                write_failed(f, failed)
            }
            PassOutcome::Failed { error } => write!(
                f,
                "outcome=failed duration_ms={duration_ms} error=\"{}\"",
                one_line(error)
            ),
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
    use chrono::TimeZone;
    use rstest::rstest;

    use super::*;
    use workflows::known_secrets::KnownSecrets;

    fn report(outcome: PassOutcome) -> PassReport {
        PassReport {
            at: Utc.with_ymd_and_hms(2026, 10, 3, 23, 15, 55).unwrap(),
            pid: 4242,
            profile: "dev".to_string(),
            duration: Duration::from_millis(6412),
            outcome,
        }
    }

    /// Sources that failed, each as `(name, error)`.
    fn failed(failures: &[(&str, &str)]) -> Vec<SourceFailure> {
        failures
            .iter()
            .map(|(source, error)| {
                SourceFailure::new(
                    *source,
                    &anyhow::anyhow!((*error).to_string()),
                    &KnownSecrets::new(Vec::new()),
                )
            })
            .collect()
    }

    #[rstest]
    #[case::ok(
        PassOutcome::Wrote { items: 1066, failed: vec![] },
        "outcome=ok duration_ms=6412 items=1066 failed_sources=0"
    )]
    #[case::partial(
        PassOutcome::Wrote {
            items: 1066,
            failed: failed(&[
                ("private workflows", "connection refused"),
                ("loki (app · prod)", "did not answer within 60s"),
            ]),
        },
        "outcome=partial duration_ms=6412 items=1066 failed_sources=2 failed=\"private workflows: connection refused; loki (app · prod): did not answer within 60s\""
    )]
    #[case::unchanged(
        PassOutcome::Unchanged { failed: failed(&[("github issues", "failed to reach GitHub API: dns error")]) },
        "outcome=unchanged duration_ms=6412 failed_sources=1 failed=\"github issues: failed to reach GitHub API: dns error\""
    )]
    #[case::failed(
        PassOutcome::Failed { error: "database is locked".to_string() },
        "outcome=failed duration_ms=6412 error=\"database is locked\""
    )]
    fn each_pass_is_one_line_saying_what_it_did(#[case] outcome: PassOutcome, #[case] tail: &str) {
        let line = report(outcome).to_string();

        assert_eq!(
            line,
            format!("hub-daemon pass at=2026-10-03T23:15:55Z pid=4242 profile=dev {tail}")
        );
    }

    #[test]
    fn a_line_stays_one_line_whatever_the_error_says() {
        let line = report(PassOutcome::Failed {
            error: "first\nsecond \"quoted\"".to_string(),
        })
        .to_string();

        assert!(!line.contains('\n'), "{line}");
        assert!(line.ends_with("error=\"first second 'quoted'\""), "{line}");
    }

    #[rstest]
    #[case::wrote(PassOutcome::Wrote { items: 0, failed: vec![] }, true)]
    #[case::unchanged(PassOutcome::Unchanged { failed: vec![] }, false)]
    #[case::failed(PassOutcome::Failed { error: String::new() }, false)]
    fn once_succeeds_only_when_the_cache_was_written(
        #[case] outcome: PassOutcome,
        #[case] wrote: bool,
    ) {
        assert_eq!(report(outcome).wrote_the_cache(), wrote);
    }
}
