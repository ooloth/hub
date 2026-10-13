//! Why the daemon could not obtain its credentials from 1Password, in a form safe to log and store.
use crate::known_secrets::KnownSecrets;
use crate::source_failure::FailureReason;

/// What kept `op` from returning a credential, as far as its message says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialCause {
    /// 1Password's servers could not be reached. Usually clears on its own once the network does.
    Unreachable,
    /// The 1Password desktop app is not running, or has not finished starting. Clears once it is
    /// open.
    AppNotRunning,
    /// Anything `op` said that hub does not recognise. The reason keeps its words.
    Other,
}

impl CredentialCause {
    /// The word the log line uses.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unreachable => "unreachable",
            Self::AppNotRunning => "app_not_running",
            Self::Other => "other",
        }
    }

    /// The cause `op`'s message names, matched without regard to case.
    ///
    /// The desktop app is checked first, because its message comes from a failed connection
    /// too. A message that names neither is [`Self::Other`], never a guess.
    fn named_in(message: &str) -> Self {
        let message = message.to_lowercase();
        let names = |markers: &[&str]| markers.iter().any(|marker| message.contains(marker));
        if names(APP_NOT_RUNNING) {
            Self::AppNotRunning
        } else if names(UNREACHABLE) {
            Self::Unreachable
        } else {
            Self::Other
        }
    }
}

/// What `op` says when it cannot reach the desktop app, lowercased.
const APP_NOT_RUNNING: &[&str] = &["couldn't connect to the 1password desktop app"];

/// What Go's network stack, which `op` is built on, says when a server cannot be reached,
/// lowercased.
const UNREACHABLE: &[&str] = &[
    "dial tcp",
    "no such host",
    "i/o timeout",
    "tls handshake timeout",
    "network is unreachable",
];

/// Why one attempt to obtain the daemon's credentials failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialFailure {
    cause: CredentialCause,
    reason: FailureReason,
}

impl CredentialFailure {
    /// The failure `error` describes, its reason redacted of every value in `secrets`: the
    /// credentials the same attempt had already resolved.
    ///
    /// # Panics
    /// If the redacted reason still holds a known secret, spans lines, or is too long. Only a bug
    /// in redaction can cause any of these.
    #[must_use]
    pub fn new(error: &anyhow::Error, secrets: &KnownSecrets) -> Self {
        let reason = FailureReason::redacted(error, secrets, "the credentials");
        let cause = CredentialCause::named_in(reason.as_str());
        Self { cause, reason }
    }

    /// What kept the credential from being returned.
    #[must_use]
    pub const fn cause(&self) -> CredentialCause {
        self.cause
    }

    /// The error, as `op` and hub described it.
    #[must_use]
    pub const fn reason(&self) -> &FailureReason {
        &self.reason
    }
}

#[cfg(test)]
mod tests {
    use anyhow::anyhow;
    use proptest::prelude::*;
    use rstest::rstest;

    use super::*;
    use crate::source_failure::hidden_secrets;

    fn no_secrets() -> KnownSecrets {
        KnownSecrets::new(Vec::new())
    }

    /// `op`'s stderr as hub wraps it when `op read` exits non-zero.
    fn from_op(stderr: &str) -> CredentialFailure {
        let error = anyhow!("failed to resolve credential from 1Password: {stderr}");
        CredentialFailure::new(&error, &no_secrets())
    }

    #[rstest]
    // Measured with op 2.34.0, 2026-10-02, with the desktop app quit.
    #[case::app_quit(
        "[ERROR] 2026/10/02 20:11:04 could not read secret 'op://Scripts/GitHub/token': error \
         initializing client: connecting to desktop app: 1Password CLI couldn't connect to the \
         1Password desktop app."
    )]
    #[case::any_case("1PASSWORD CLI COULDN'T CONNECT TO THE 1PASSWORD DESKTOP APP")]
    fn a_message_naming_the_desktop_app_is_the_app_not_running(#[case] stderr: &str) {
        assert_eq!(from_op(stderr).cause(), CredentialCause::AppNotRunning);
    }

    #[rstest]
    // Measured with op 2.34.0, 2026-09-19, with egress pointed at a dead port.
    #[case::proxy_refused(
        "could not read secret 'op://Scripts/GitHub/token': error initializing client: Get \
         \"https://my.1password.com/api/v2/account/keysets?__t=1\": proxyconnect tcp: dial tcp \
         127.0.0.1:1: connect: connection refused"
    )]
    // Measured with op 2.40.0, 2026-10-12, with egress pointed at a dead port.
    #[case::proxy_refused_2_40(
        "[ERROR] 2026/10/12 21:11:39 could not read secret \
         'op://Scripts/GitHub/personal-access-token-hub': error initializing client: Get \
         \"/api/v2/account/keysets\": proxyconnect tcp: dial tcp 127.0.0.1:1: connect: \
         connection refused"
    )]
    #[case::no_dns(
        "error initializing client: Get \"https://my.1password.com/api/v2/auth\": dial tcp: \
         lookup my.1password.com: no such host"
    )]
    #[case::timeout(
        "error initializing client: Get \"https://my.1password.com/api/v2/auth\": net/http: TLS \
         handshake timeout"
    )]
    #[case::read_timeout("Post \"https://my.1password.com/api/v3/auth\": read tcp: i/o timeout")]
    #[case::offline("dial tcp 10.0.0.1:443: connect: network is unreachable")]
    fn a_network_error_is_1password_unreachable(#[case] stderr: &str) {
        assert_eq!(from_op(stderr).cause(), CredentialCause::Unreachable);
    }

    #[rstest]
    #[case::bad_reference("could not read secret 'op://Nope/x/y': \"Nope\" isn't a vault")]
    #[case::no_op("failed to run `op read`: No such file or directory (os error 2)")]
    fn any_other_message_is_other(#[case] stderr: &str) {
        assert_eq!(from_op(stderr).cause(), CredentialCause::Other);
    }

    #[test]
    fn the_reason_keeps_ops_words() {
        let failure = from_op("could not read secret 'op://Nope/x/y': \"Nope\" isn't a vault");

        assert_eq!(
            failure.reason().as_str(),
            "failed to resolve credential from 1Password: could not read secret \
             'op://Nope/x/y': \"Nope\" isn't a vault"
        );
    }

    /// Words drawn from no marker of a named cause.
    fn unrecognised_message() -> impl Strategy<Value = String> {
        "[a-z ':./\"0-9]{0,120}".prop_filter("names a cause", |message| {
            let lower = message.to_lowercase();
            ![
                "desktop app",
                "dial tcp",
                "no such host",
                "timeout",
                "unreachable",
            ]
            .iter()
            .any(|marker| lower.contains(marker))
        })
    }

    proptest! {
        #[test]
        fn an_unrecognised_message_is_other_and_kept(message in unrecognised_message()) {
            let failure = from_op(&message);

            prop_assert_eq!(failure.cause(), CredentialCause::Other);
            let kept = message.split_whitespace().collect::<Vec<_>>().join(" ");
            prop_assert!(failure.reason().as_str().contains(&kept), "{}", failure.reason());
        }

        #[test]
        fn no_resolved_credential_survives_into_the_reason(
            hidden in hidden_secrets::strategy()
        ) {
            let failure = CredentialFailure::new(&hidden.error(), &hidden.secrets());
            let text = failure.reason().as_str();

            prop_assert!(hidden.survives_in(text).is_none(), "{:?} in {}", hidden.secret, text);
        }
    }
}
