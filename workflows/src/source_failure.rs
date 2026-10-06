//! A source that failed during a refresh, and why, in a form safe to log and store.
use std::fmt;

use crate::known_secrets::KnownSecrets;

/// The most characters one link of an error chain keeps. A client can put a whole response body
/// into one link, and this keeps the start of it: the status and the first of the body.
const LINK_LIMIT: usize = 300;

/// The most characters a whole reason keeps. A chain longer than this is cut from the middle, so
/// the outermost context and the root cause both survive.
const REASON_LIMIT: usize = 1000;

/// What marks text that was cut.
const CUT: char = '…';

/// A source that failed during a refresh, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFailure {
    source: String,
    reason: FailureReason,
}

impl SourceFailure {
    /// The failure of `source` with `error`, its reason redacted of every secret in `secrets`.
    pub fn new(source: impl Into<String>, error: &anyhow::Error, secrets: &KnownSecrets) -> Self {
        let source = source.into();
        let reason = FailureReason::redacted(error, secrets, &source);
        Self { source, reason }
    }

    /// The source's name, as the report names it.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Why it failed.
    #[must_use]
    pub const fn reason(&self) -> &FailureReason {
        &self.reason
    }
}

/// Why a source failed: its error chain, outermost context first, on one line, at most
/// [`REASON_LIMIT`] characters, with every known secret replaced.
///
/// The only constructor redacts, so a reason that holds a known secret cannot be made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailureReason(String);

impl FailureReason {
    /// Each link of `error`'s chain on one line, redacted, then cut to [`LINK_LIMIT`]; the links
    /// joined as anyhow joins them; the whole cut to [`REASON_LIMIT`] from the middle.
    ///
    /// Redaction comes before every cut, so a cut cannot leave the start of a secret behind.
    fn redacted(error: &anyhow::Error, secrets: &KnownSecrets, source: &str) -> Self {
        let links: Vec<String> = error
            .chain()
            .map(|link| keep_start(&secrets.redact(&on_one_line(&link.to_string())), LINK_LIMIT))
            .collect();
        let reason = keep_both_ends(&links.join(": "), REASON_LIMIT);

        secrets.assert_absent_from(&reason, source);
        assert!(
            !reason.contains('\n'),
            "the failure reason for {source} spans lines"
        );
        assert!(
            reason.chars().count() <= REASON_LIMIT,
            "the failure reason for {source} is longer than {REASON_LIMIT} characters"
        );
        Self(reason)
    }

    /// The reason as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for FailureReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// `text` cut to its first `limit` characters, the last of them marking the cut.
fn keep_start(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut kept: String = text.chars().take(limit.saturating_sub(1)).collect();
    kept.push(CUT);
    kept
}

/// `text` cut to `limit` characters by removing its middle, so its start and its end survive.
fn keep_both_ends(text: &str, limit: usize) -> String {
    let length = text.chars().count();
    if length <= limit {
        return text.to_string();
    }
    let head = limit / 2;
    let tail = limit.saturating_sub(head).saturating_sub(1);
    let start: String = text.chars().take(head).collect();
    let end: String = text.chars().skip(length.saturating_sub(tail)).collect();
    format!("{start}{CUT}{end}")
}

/// `text` with every run of whitespace, line breaks included, collapsed to one space.
pub(crate) fn on_one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use anyhow::{anyhow, Context};
    use proptest::prelude::*;

    use super::*;
    use crate::known_secrets::REDACTED;

    fn no_secrets() -> KnownSecrets {
        KnownSecrets::new(Vec::new())
    }

    fn reason(error: &anyhow::Error, secrets: &KnownSecrets) -> String {
        SourceFailure::new("source", error, secrets)
            .reason()
            .as_str()
            .to_string()
    }

    #[test]
    fn a_reason_keeps_the_whole_chain_outermost_first() {
        let error = Err::<(), _>(anyhow!("dns error"))
            .context("error sending request")
            .context("failed to reach GitHub API")
            .unwrap_err();

        assert_eq!(
            reason(&error, &no_secrets()),
            "failed to reach GitHub API: error sending request: dns error"
        );
    }

    #[test]
    fn a_reason_is_one_line_whatever_the_error_says() {
        let error = anyhow!("first line\nsecond line\r\n\tthird");

        assert_eq!(
            reason(&error, &no_secrets()),
            "first line second line third"
        );
    }

    #[test]
    fn a_long_body_is_cut_within_its_own_link() {
        let body = "x".repeat(5000);
        let error = Err::<(), _>(anyhow!("Linear API error 500: {body}"))
            .context("failed to fetch Linear issues")
            .unwrap_err();

        let text = reason(&error, &no_secrets());

        let (outer, link) = text.split_once(": ").unwrap();
        assert_eq!(outer, "failed to fetch Linear issues");
        assert!(link.starts_with("Linear API error 500: xxx"), "{link}");
        assert_eq!(link.chars().count(), LINK_LIMIT);
        assert!(link.ends_with(CUT), "{link}");
    }

    #[test]
    fn a_deep_chain_is_cut_from_the_middle_keeping_both_ends() {
        let error = (1..10).fold(anyhow!("root cause {}", "r".repeat(250)), |error, depth| {
            error.context(format!("context {depth} {}", "c".repeat(250)))
        });

        let text = reason(&error, &no_secrets());

        assert!(
            text.chars().count() <= REASON_LIMIT,
            "{}",
            text.chars().count()
        );
        assert!(text.starts_with("context 9 "), "{text}");
        assert!(text.contains(CUT), "{text}");
        assert!(text.ends_with(&"r".repeat(100)), "{text}");
    }

    #[test]
    fn the_host_of_a_credential_url_is_removed_from_a_request_error() {
        let secrets = KnownSecrets::new(vec!["https://media.internal:8989".to_string()]);
        let error = Err::<(), _>(anyhow!(
            "error sending request for url (https://media.internal:8989/api/v3/queue?page=1)"
        ))
        .context("failed to reach https://media.internal:8989/api/v3/queue")
        .unwrap_err();

        let text = reason(&error, &secrets);

        assert!(!text.contains("media.internal"), "{text}");
        assert!(text.contains(REDACTED), "{text}");
    }

    #[test]
    fn a_secret_where_a_link_is_cut_leaves_none_of_itself_behind() {
        let secret = "s3cret-token-value-0123";
        let secrets = KnownSecrets::new(vec![secret.to_string()]);
        let error = anyhow!("{}{secret}", "x".repeat(LINK_LIMIT - 10));

        let text = reason(&error, &secrets);

        assert!(!text.contains("s3cret"), "{text}");
    }

    proptest! {
        #[test]
        fn no_known_secret_survives_into_a_reason(
            secret in "[a-zA-Z0-9.*+?()|^$-]{4,24}",
            contained in 1_usize..4,
            before in "[ -~\n]{0,40}",
            between in "[ -~\n]{0,40}",
            after in "[ -~\n]{0,40}",
        ) {
            let shorter: String = secret.chars().take(contained).collect();
            let secrets = KnownSecrets::new(vec![secret.clone(), shorter]);
            let error = Err::<(), _>(anyhow!("{before}{secret}\n{between}{secret}{after}"))
                .context(format!("failed to reach {secret}"))
                .unwrap_err();

            let text = reason(&error, &secrets);

            for piece in text.split(REDACTED) {
                prop_assert!(!piece.contains(secret.as_str()), "{} in {}", secret, text);
            }
        }
    }
}
