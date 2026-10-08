//! Why a whole refresh pass failed, in a form safe to log and store.
use crate::known_secrets::KnownSecrets;
use crate::source_failure::FailureReason;

/// Why a pass failed as a whole rather than one of its sources: the refresh could not run, the
/// payload could not be serialized, or the database could not be written.
///
/// Its reason is made the same way as a source's, so it holds no known secret, spans one line and
/// is at most as long as a source's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PassFailure(FailureReason);

impl PassFailure {
    /// The pass failed with `error`, redacted of every value in `secrets`.
    ///
    /// # Panics
    /// If the redacted reason still holds a known secret, spans lines, or is too long. Only a bug
    /// in redaction can cause any of these.
    #[must_use]
    pub fn new(error: &anyhow::Error, secrets: &KnownSecrets) -> Self {
        Self(FailureReason::redacted(error, secrets, "the pass"))
    }

    /// Why the pass failed.
    #[must_use]
    pub const fn reason(&self) -> &FailureReason {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::source_failure::hidden_secrets;

    proptest! {
        #[test]
        fn no_known_secret_survives_into_a_pass_failure(hidden in hidden_secrets::strategy()) {
            let failure = PassFailure::new(&hidden.error(), &hidden.secrets());
            let text = failure.reason().as_str();

            prop_assert!(hidden.survives_in(text).is_none(), "{:?} in {}", hidden.secret, text);
        }
    }

    #[test]
    fn a_pass_failure_keeps_the_whole_chain_on_one_line() {
        let error = anyhow::anyhow!("disk I/O error\nretry later").context("failed to write");

        let failure = PassFailure::new(&error, &KnownSecrets::new(Vec::new()));

        assert_eq!(
            failure.reason().as_str(),
            "failed to write: disk I/O error retry later"
        );
    }
}
