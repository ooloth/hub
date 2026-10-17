//! The notification posted when the daemon has gone without credentials for a while.
//!
//! It says that signals have paused, and what is needed to bring them back.
use crate::credential_failure::CredentialCause;

/// The title every such notification carries.
pub const TITLE: &str = "hub: signals paused";

/// A notification to post: its title and its body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    /// What it is about.
    pub title: String,
    /// What is needed.
    pub body: String,
}

impl Notification {
    /// The notification for an attempt that failed with `cause`: what someone at the machine would
    /// have to do.
    #[must_use]
    pub fn for_cause(cause: CredentialCause) -> Self {
        let body = match cause {
            CredentialCause::MacosPromptWaitingForYou => {
                "Approve the macOS prompt: hub-daemon would like to access data from other apps."
            }
            CredentialCause::TouchIdPromptUnanswered => {
                "Approve 1Password's Touch ID prompt for hub-daemon."
            }
            CredentialCause::AppRunningNotAnswering => {
                "1Password isn't answering. If it keeps happening, quit and reopen 1Password."
            }
            CredentialCause::AppNotRunning => "1Password isn't running. Open 1Password.",
            CredentialCause::Unreachable => {
                "1Password can't be reached. Check the internet connection."
            }
            CredentialCause::Other => "Couldn't read a credential. See the hub-daemon log.",
        };
        Self {
            title: TITLE.to_string(),
            body: body.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::macos_prompt(
        CredentialCause::MacosPromptWaitingForYou,
        "Approve the macOS prompt: hub-daemon would like to access data from other apps."
    )]
    #[case::touch_id(
        CredentialCause::TouchIdPromptUnanswered,
        "Approve 1Password's Touch ID prompt for hub-daemon."
    )]
    #[case::app_running(
        CredentialCause::AppRunningNotAnswering,
        "1Password isn't answering. If it keeps happening, quit and reopen 1Password."
    )]
    #[case::app_not_running(
        CredentialCause::AppNotRunning,
        "1Password isn't running. Open 1Password."
    )]
    #[case::unreachable(
        CredentialCause::Unreachable,
        "1Password can't be reached. Check the internet connection."
    )]
    #[case::other(
        CredentialCause::Other,
        "Couldn't read a credential. See the hub-daemon log."
    )]
    fn each_cause_says_what_is_needed(#[case] cause: CredentialCause, #[case] body: &str) {
        let notification = Notification::for_cause(cause);

        assert_eq!(notification.title, "hub: signals paused");
        assert_eq!(notification.body, body);
    }
}
