//! Why the daemon could not obtain its credentials from 1Password, in a form safe to log and store.
use std::time::Duration;

use crate::known_secrets::KnownSecrets;
use crate::op_spawn::OpSpawn;
use crate::screen_at_deadline::ScreenAtDeadline;
use crate::source_failure::FailureReason;

/// What kept `op` from returning a credential: what its message says, or that it said nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialCause {
    /// 1Password's servers could not be reached. Usually clears on its own once the network does.
    Unreachable,
    /// The 1Password desktop app is not running, or has not finished starting. Clears once it is
    /// open.
    AppNotRunning,
    /// 1Password's Touch ID prompt was on screen and nobody approved it, so `op` gave up after 60
    /// seconds. Clears once someone approves the next prompt.
    TouchIdPromptUnanswered,
    /// `op` had not answered by its deadline and a macOS alert was on screen, waiting for someone.
    MacosPromptWaitingForYou,
    /// `op` had not answered by its deadline, no macOS alert was on screen, and the 1Password app
    /// was running: the app is what was not answering. Also recorded when the screen could not be
    /// checked, with the reason saying so.
    AppRunningNotAnswering,
    /// `op`'s CLI integration can see no 1Password account. Either "Integrate with 1Password CLI"
    /// is off in the desktop app, or `op` answers to a process 1Password does not trust. The
    /// daemon answers this by reading the credential again through an `op` that answers for
    /// itself, so it reaches the log only when that read is refused too.
    NoAccounts,
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
            Self::TouchIdPromptUnanswered => "touch_id_prompt_unanswered",
            Self::MacosPromptWaitingForYou => "macos_prompt_waiting_for_you",
            Self::AppRunningNotAnswering => "app_running_not_answering",
            Self::NoAccounts => "no_accounts",
            Self::Other => "other",
        }
    }

    /// The cause `op`'s message names, matched without regard to case.
    ///
    /// The desktop app and the unanswered prompt are checked before an unreachable server, because
    /// their messages come from a failed connection or a timeout too. No accounts is checked
    /// first; its message shares no words with the others, so its place decides nothing today. A
    /// message that names none is [`Self::Other`],
    /// never a guess. The causes a deadline finds are never named here: only the deadline and the
    /// screen know them.
    fn named_in(message: &str) -> Self {
        let message = message.to_lowercase();
        let names = |markers: &[&str]| markers.iter().any(|marker| message.contains(marker));
        if names(NO_ACCOUNTS) {
            Self::NoAccounts
        } else if names(APP_NOT_RUNNING) {
            Self::AppNotRunning
        } else if names(TOUCH_ID_PROMPT_UNANSWERED) {
            Self::TouchIdPromptUnanswered
        } else if names(UNREACHABLE) {
            Self::Unreachable
        } else {
            Self::Other
        }
    }
}

/// What `op` says when its CLI integration can see no 1Password account, lowercased. Measured with
/// `op` 2.41.1 on 2026-11-01.
const NO_ACCOUNTS: &[&str] = &["no accounts configured for use with 1password cli"];

/// What `op` says when it cannot reach the desktop app, lowercased.
const APP_NOT_RUNNING: &[&str] = &["couldn't connect to the 1password desktop app"];

/// What `op` says when it gave up on a Touch ID prompt nobody answered, lowercased.
const TOUCH_ID_PROMPT_UNANSWERED: &[&str] = &["authorization timeout"];

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
    spawn: OpSpawn,
}

impl CredentialFailure {
    /// The failure `error` describes, from an `op` started as `spawn`, its reason redacted of
    /// every value in `secrets`: the credentials the same attempt had already resolved.
    ///
    /// The cause is read from `error`'s own words before they are redacted or cut, so an earlier
    /// credential that occurs inside `op`'s message, or a long message, cannot hide it. Only the
    /// cause is kept from those words.
    ///
    /// # Panics
    /// If the redacted reason still holds a known secret, spans lines, or is too long. Only a bug
    /// in redaction can cause any of these.
    #[must_use]
    pub fn new(error: &anyhow::Error, secrets: &KnownSecrets, spawn: OpSpawn) -> Self {
        let cause = CredentialCause::named_in(&format!("{error:#}"));
        let reason = FailureReason::redacted(error, secrets, "the credentials");
        Self {
            cause,
            reason,
            spawn,
        }
    }

    /// The `op` started as `spawn` had not answered for `reference` after `after`, so it was
    /// ended, and `screen` is what was then on screen. Known from the deadline and the screen,
    /// never read back from text.
    ///
    /// # Panics
    /// As [`Self::new`].
    #[must_use]
    pub fn no_answer(
        reference: &str,
        after: Duration,
        screen: &ScreenAtDeadline,
        secrets: &KnownSecrets,
        spawn: OpSpawn,
    ) -> Self {
        let error = anyhow::anyhow!(
            "1Password did not answer within {}s reading {reference}; {}",
            after.as_secs(),
            screen.describe()
        );
        Self {
            cause: screen.cause(),
            reason: FailureReason::redacted(&error, secrets, "the credentials"),
            spawn,
        }
    }

    /// What kept the credential from being returned.
    #[must_use]
    pub const fn cause(&self) -> CredentialCause {
        self.cause
    }

    /// How the `op` that failed was started.
    #[must_use]
    pub const fn spawn(&self) -> OpSpawn {
        self.spawn
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
        CredentialFailure::new(&error, &no_secrets(), OpSpawn::Inherited)
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

    /// What `op` 2.41.1 said on macOS 27.2 under launchd, on one line.
    const REFUSED: &str = "No accounts configured for use with 1Password CLI. - Turn on the \
                           1Password desktop app integration";

    #[test]
    fn a_refusal_whose_words_are_cut_from_the_reason_is_still_no_accounts() {
        let error = anyhow!(
            "failed to resolve credential from 1Password: [ERROR] could not read secret \
             'op://{}': {REFUSED}",
            "a".repeat(300)
        );

        let failure = CredentialFailure::new(&error, &no_secrets(), OpSpawn::Inherited);

        assert!(!failure.reason().as_str().contains("No accounts"));
        assert_eq!(failure.cause(), CredentialCause::NoAccounts);
    }

    #[rstest]
    // Measured with op 2.41.1 on macOS 27.2, 2026-11-01, under launchd with an ad-hoc-signed parent.
    #[case::macos_27_launchd(
        "No accounts configured for use with 1Password CLI.\n\n - Turn on the 1Password desktop \
         app integration to sign in with the accounts you've added to the app: \
         https://www.1password.dev/cli/app-integration/ for details.\n[ERROR] 2026/11/01 16:48:54 \
         could not read secret 'op://Scripts/GitHub/token': error initializing client: "
    )]
    #[case::any_case("NO ACCOUNTS CONFIGURED FOR USE WITH 1PASSWORD CLI")]
    fn a_message_saying_no_accounts_are_configured_is_no_accounts(#[case] stderr: &str) {
        assert_eq!(from_op(stderr).cause(), CredentialCause::NoAccounts);
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

    // Measured with op 2.40.0, 2026-10-17, with the vault locked and the prompt left unanswered.
    #[test]
    fn an_authorization_timeout_is_a_touch_id_prompt_nobody_answered() {
        let failure = from_op(
            "[ERROR] 2026/10/17 10:25:39 could not read secret \
             'op://Scripts/GitHub/personal-access-token-hub': error initializing client: \
             authorization timeout",
        );

        assert_eq!(failure.cause(), CredentialCause::TouchIdPromptUnanswered);
    }

    #[test]
    fn no_answer_takes_its_cause_from_the_screen_and_names_reference_deadline_and_screen() {
        let failure = CredentialFailure::no_answer(
            "op://Scripts/GitHub/token",
            Duration::from_secs(90),
            &ScreenAtDeadline::MacosAlert { windows: 1 },
            &no_secrets(),
            OpSpawn::Disclaimed,
        );

        assert_eq!(failure.cause(), CredentialCause::MacosPromptWaitingForYou);
        assert_eq!(failure.spawn(), OpSpawn::Disclaimed);
        assert_eq!(
            failure.reason().as_str(),
            "1Password did not answer within 90s reading op://Scripts/GitHub/token; 1 macOS \
             alert window(s) from UserNotificationCenter on screen"
        );
    }

    #[test]
    fn hubs_own_words_for_a_deadline_are_not_read_back_as_its_cause() {
        let failure = from_op(
            "1Password did not answer within 90s reading op://x/y/z; 1 macOS alert window(s) from \
             UserNotificationCenter on screen",
        );

        assert_eq!(failure.cause(), CredentialCause::Other);
    }

    #[rstest]
    #[case::bad_reference("could not read secret 'op://Nope/x/y': \"Nope\" isn't a vault")]
    #[case::no_op(
        "`op` is not on the PATH; is the 1Password CLI installed?: No such file or directory (os \
         error 2)"
    )]
    #[case::no_disclaim(
        "macOS cannot start `op` so that it answers for itself: \
         responsibility_spawnattrs_setdisclaim is not available on this macOS, so the child cannot \
         answer for itself"
    )]
    #[case::other_start_failure("failed to start `op read`: Too many open files (os error 24)")]
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
                "no accounts",
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
            let failure =
                CredentialFailure::new(&hidden.error(), &hidden.secrets(), OpSpawn::Inherited);
            let text = failure.reason().as_str();

            prop_assert!(hidden.survives_in(text).is_none(), "{:?} in {}", hidden.secret, text);
        }

        #[test]
        fn a_refusal_is_no_accounts_whatever_earlier_credential_occurs_in_it(
            (start, length) in (0..REFUSED.len()).prop_flat_map(|start| {
                (Just(start), 1..=REFUSED.len() - start)
            })
        ) {
            let earlier = REFUSED[start..start + length].to_string();
            let error = anyhow!("failed to resolve credential from 1Password: {REFUSED}");

            let failure = CredentialFailure::new(
                &error,
                &KnownSecrets::new(vec![earlier]),
                OpSpawn::Inherited,
            );

            prop_assert_eq!(failure.cause(), CredentialCause::NoAccounts);
        }
    }
}
