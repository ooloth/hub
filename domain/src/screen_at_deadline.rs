//! What the screen and the 1Password app showed when an `op read` reached its deadline: who was
//! not answering.
use crate::credential_failure::CredentialCause;

/// The program that owns every macOS alert window, the "access data from other apps" dialog among
/// them. Measured on 2026-10-17: that dialog was a window owned by it, on layer 8.
pub const MACOS_ALERT_OWNER: &str = "UserNotificationCenter";

/// The program that owns 1Password's Touch ID prompt.
///
/// The prompt is a window above the normal layer, and the app's ordinary windows have the same owner
/// on layer 0. Measured on 2026-10-17: the prompt was on layer 101.
pub const TOUCH_ID_OWNER: &str = "1Password";

/// One window on screen: which program owns it, and its layer. Layer 0 is ordinary app windows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnScreenWindow {
    /// The owning program's name.
    pub owner: String,
    /// The window's layer.
    pub layer: i64,
}

impl OnScreenWindow {
    fn is_macos_alert(&self) -> bool {
        self.owner == MACOS_ALERT_OWNER
    }

    fn is_touch_id_prompt(&self) -> bool {
        self.owner == TOUCH_ID_OWNER && self.layer > 0
    }
}

/// What was on screen, and whether the 1Password app was running, when `op` had not answered by
/// its deadline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScreenAtDeadline {
    /// 1Password's Touch ID prompt was on screen, so `op` was waiting for someone to approve it and
    /// will give up on its own within a minute.
    TouchIdPrompt,
    /// At least one macOS alert was on screen, waiting for someone. Which alert is not known:
    /// telling them apart needs the window's title, which needs the Screen Recording permission.
    MacosAlert {
        /// How many windows `UserNotificationCenter` had on screen.
        windows: usize,
    },
    /// No macOS alert was on screen and the 1Password app was running, so the app is what was not
    /// answering.
    AppRunningNoAlert,
    /// No macOS alert was on screen and the 1Password app was not running.
    AppNotRunning,
    /// The window list or the process list could not be read.
    Unreadable {
        /// Why not.
        why: String,
    },
}

impl ScreenAtDeadline {
    /// What `windows`, every on-screen window, and `app_running` say. A prompt on screen comes
    /// first, so a waiting prompt is never reported as the app not answering. The Touch ID prompt
    /// comes before a macOS alert, because `op` gives up on it by itself, so the read can safely be
    /// left to finish.
    #[must_use]
    pub fn seen(windows: &[OnScreenWindow], app_running: bool) -> Self {
        let alerts = windows
            .iter()
            .filter(|window| window.is_macos_alert())
            .count();
        if windows.iter().any(OnScreenWindow::is_touch_id_prompt) {
            Self::TouchIdPrompt
        } else if alerts > 0 {
            Self::MacosAlert { windows: alerts }
        } else if app_running {
            Self::AppRunningNoAlert
        } else {
            Self::AppNotRunning
        }
    }

    /// The cause this names.
    #[must_use]
    pub const fn cause(&self) -> CredentialCause {
        match self {
            Self::TouchIdPrompt => CredentialCause::TouchIdPromptUnanswered,
            Self::MacosAlert { .. } => CredentialCause::MacosPromptWaitingForYou,
            Self::AppRunningNoAlert | Self::Unreadable { .. } => {
                CredentialCause::AppRunningNotAnswering
            }
            Self::AppNotRunning => CredentialCause::AppNotRunning,
        }
    }

    /// What was found, for the failure's reason.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::TouchIdPrompt => "1Password's Touch ID prompt is on screen".to_string(),
            Self::MacosAlert { windows } => {
                format!("{windows} macOS alert window(s) from {MACOS_ALERT_OWNER} on screen")
            }
            Self::AppRunningNoAlert => {
                "the 1Password app is running and no macOS alert is on screen".to_string()
            }
            Self::AppNotRunning => {
                "the 1Password app is not running and no macOS alert is on screen".to_string()
            }
            Self::Unreadable { why } => format!("the screen could not be checked: {why}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    /// Windows as `(owner, layer)`.
    fn windows(list: &[(&str, i64)]) -> Vec<OnScreenWindow> {
        list.iter()
            .map(|(owner, layer)| OnScreenWindow {
                owner: (*owner).to_string(),
                layer: *layer,
            })
            .collect()
    }

    #[rstest]
    // Measured 2026-10-17 under launchd: the access dialog on screen, the app running but locked.
    #[case::alert_with_the_app_running(&[("Control Center", 25), ("UserNotificationCenter", 8), ("Safari", 0)], true,
        ScreenAtDeadline::MacosAlert { windows: 1 }, CredentialCause::MacosPromptWaitingForYou)]
    #[case::alert_with_the_app_quit(&[("UserNotificationCenter", 8)], false,
        ScreenAtDeadline::MacosAlert { windows: 1 }, CredentialCause::MacosPromptWaitingForYou)]
    #[case::two_alerts(&[("UserNotificationCenter", 8), ("UserNotificationCenter", 8)], true,
        ScreenAtDeadline::MacosAlert { windows: 2 }, CredentialCause::MacosPromptWaitingForYou)]
    // Measured 2026-10-17: the Touch ID prompt is a 1Password window on layer 101.
    #[case::touch_id_prompt(&[("Control Center", 25), ("1Password", 101)], true,
        ScreenAtDeadline::TouchIdPrompt, CredentialCause::TouchIdPromptUnanswered)]
    // The app's own window is layer 0, and is not a prompt.
    #[case::the_apps_own_window(&[("1Password", 0)], true,
        ScreenAtDeadline::AppRunningNoAlert, CredentialCause::AppRunningNotAnswering)]
    // Seen 2026-10-12: op stuck after the app was reopened and unlocked, nothing on screen.
    #[case::no_alert_with_the_app_running(&[("Control Center", 25)], true,
        ScreenAtDeadline::AppRunningNoAlert, CredentialCause::AppRunningNotAnswering)]
    #[case::no_alert_with_the_app_quit(&[("Control Center", 25)], false,
        ScreenAtDeadline::AppNotRunning, CredentialCause::AppNotRunning)]
    fn who_was_not_answering_is_read_from_the_screen_and_the_app(
        #[case] on_screen: &[(&str, i64)],
        #[case] app_running: bool,
        #[case] expected: ScreenAtDeadline,
        #[case] cause: CredentialCause,
    ) {
        let seen = ScreenAtDeadline::seen(&windows(on_screen), app_running);

        assert_eq!(seen, expected);
        assert_eq!(seen.cause(), cause);
    }

    #[test]
    fn an_unreadable_screen_names_the_app_as_not_answering_and_says_why() {
        let seen = ScreenAtDeadline::Unreadable {
            why: "osascript exited 1".to_string(),
        };

        assert_eq!(seen.cause(), CredentialCause::AppRunningNotAnswering);
        assert_eq!(
            seen.describe(),
            "the screen could not be checked: osascript exited 1"
        );
    }
}
