//! The credential values a failure reason must never contain.
use crate::source_failure::on_one_line;

/// What replaces a credential wherever it appears in a failure reason.
pub(crate) const REDACTED: &str = "[redacted]";

/// Every credential a refresh was given, and every part of one that is a URL, in the form it
/// takes once a reason is written on one line.
///
/// Longest first, so a credential that contains another is replaced whole before the shorter one
/// could leave part of it behind.
pub struct KnownSecrets {
    values: Vec<String>,
}

impl KnownSecrets {
    /// The secrets in `credentials`.
    ///
    /// A credential that parses as a URL with a host also contributes its host, its host and port,
    /// its username, its password and each query value, since a client can put any of those into
    /// an error on its own: a base URL built from the host, say. An empty value contributes
    /// nothing, since config can hold one and replacing the empty string is meaningless.
    pub fn new(credentials: impl IntoIterator<Item = String>) -> Self {
        let mut values: Vec<String> = credentials
            .into_iter()
            .flat_map(|credential| {
                let mut parts = url_parts(&credential);
                parts.push(credential);
                parts
            })
            .map(|value| on_one_line(&value))
            .filter(|value| !value.is_empty())
            .collect();
        values.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
        values.dedup();
        Self { values }
    }

    /// `text` with every secret replaced by [`REDACTED`].
    ///
    /// One pass, left to right, taking the longest secret that starts at each position. A
    /// replacement is never scanned again, so a secret that happens to occur inside the marker
    /// cannot rewrite it.
    pub(crate) fn redact(&self, text: &str) -> String {
        let mut redacted = String::with_capacity(text.len());
        let mut rest = text;
        // Each turn consumes at least one character, since no secret is empty.
        while let Some(next) = rest.chars().next() {
            let consumed = if let Some(secret) = self
                .values
                .iter()
                .find(|secret| rest.starts_with(secret.as_str()))
            {
                redacted.push_str(REDACTED);
                secret.len()
            } else {
                redacted.push(next);
                next.len_utf8()
            };
            rest = rest.get(consumed..).unwrap_or_default();
        }
        redacted
    }

    /// Halts if any secret appears in `text`, a reason already redacted for `source`.
    ///
    /// Only a bug in redaction can make this fire, and writing the reason anyway would put a
    /// credential into the log.
    pub(crate) fn assert_absent_from(&self, text: &str, source: &str) {
        // The marker is not the source's text, so it is not searched: a secret that happens to
        // occur inside "[redacted]" has not leaked.
        for written in text.split(REDACTED) {
            for (index, secret) in self.values.iter().enumerate() {
                assert!(
                    !written.contains(secret.as_str()),
                    "the failure reason for {source} still holds known secret #{index}"
                );
            }
        }
    }
}

/// The parts of `credential` a client could write on their own, when it is a URL with a host:
/// the host, the host and port, the username, the password and each query value.
fn url_parts(credential: &str) -> Vec<String> {
    let Ok(url) = url::Url::parse(credential) else {
        return vec![];
    };
    let Some(host) = url.host_str() else {
        return vec![];
    };
    let mut parts = vec![host.to_string(), url.username().to_string()];
    parts.extend(url.port().map(|port| format!("{host}:{port}")));
    parts.extend(url.password().map(str::to_string));
    parts.extend(url.query_pairs().map(|(_, value)| value.into_owned()));
    parts
}

impl std::fmt::Debug for KnownSecrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KnownSecrets")
            .field("count", &self.values.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn secrets(values: &[&str]) -> KnownSecrets {
        KnownSecrets::new(values.iter().map(|value| (*value).to_string()))
    }

    #[test]
    fn a_credential_is_replaced_wherever_it_appears() {
        let redacted = secrets(&["tok3n"]).redact("sent tok3n, then tok3n again");

        assert_eq!(redacted, "sent [redacted], then [redacted] again");
    }

    #[test]
    fn a_credential_that_contains_another_is_replaced_whole() {
        let redacted = secrets(&["abc", "abcdef"]).redact("key abcdef");

        assert_eq!(redacted, "key [redacted]");
    }

    #[test]
    fn an_empty_credential_replaces_nothing() {
        let redacted = secrets(&[""]).redact("nothing secret here");

        assert_eq!(redacted, "nothing secret here");
    }

    #[test]
    fn a_credential_spanning_lines_is_found_once_the_text_is_on_one_line() {
        let redacted = secrets(&["line one\nline two"]).redact("key line one line two");

        assert_eq!(redacted, "key [redacted]");
    }

    #[rstest]
    #[case::host("error sending request for url (http://media.internal/api/v3/queue)")]
    #[case::host_and_port("failed to reach media.internal:8989")]
    #[case::username("authenticating as admin-user")]
    #[case::password("rejected s3cret-pass")]
    #[case::query_value("bad key qv-9f8e7d")]
    fn each_part_of_a_credential_url_is_replaced(#[case] text: &str) {
        let known = secrets(&["https://admin-user:s3cret-pass@media.internal:8989/?key=qv-9f8e7d"]);

        let redacted = known.redact(text);

        for part in ["media.internal", "admin-user", "s3cret-pass", "qv-9f8e7d"] {
            assert!(!redacted.contains(part), "{part} survived in {redacted}");
        }
    }

    #[test]
    fn a_credential_that_is_not_a_url_is_replaced_only_whole() {
        let redacted = secrets(&["ghp_abc123"]).redact("token ghp_abc123 for ghp_");

        assert_eq!(redacted, "token [redacted] for ghp_");
    }

    #[test]
    fn its_debug_output_never_shows_a_credential() {
        let shown = format!("{:?}", secrets(&["tok3n"]));

        assert!(!shown.contains("tok3n"), "{shown}");
    }
}
