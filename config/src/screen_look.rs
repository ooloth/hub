//! What is on screen when an `op read` reaches its deadline, and whether the 1Password app is
//! running, so a failure can name who was not answering.
use std::time::Duration;

use anyhow::{Context, Result};
use domain::screen_at_deadline::{OnScreenWindow, ScreenAtDeadline};

use crate::op_process::killed_on_drop;

/// How long each look at the screen or the process list may take.
const LOOK_DEADLINE: Duration = Duration::from_secs(5);

/// Who was not answering when an `op read` reached its deadline: whether a macOS alert is on screen,
/// and whether the 1Password app is running.
pub(crate) async fn look_at_screen_and_app() -> ScreenAtDeadline {
    screen_from(on_screen_windows().await, app_running().await)
}

/// What the two looks say, or [`ScreenAtDeadline::Unreadable`] when either failed.
fn screen_from(
    windows: Result<Vec<OnScreenWindow>>,
    app_running: Result<bool>,
) -> ScreenAtDeadline {
    match (windows, app_running) {
        (Ok(windows), Ok(running)) => ScreenAtDeadline::seen(&windows, running),
        (Err(error), _) | (_, Err(error)) => ScreenAtDeadline::Unreadable {
            why: format!("{error:#}"),
        },
    }
}

/// Lists every on-screen window through `osascript`'s JavaScript bridge to CoreGraphics: its owner
/// and layer, tab-separated, one window a line. Needs no permission.
const WINDOW_OWNERS_SCRIPT: &str = r#"ObjC.import("CoreGraphics"); ObjC.import("Foundation");
const list = ObjC.castRefToObject($.CGWindowListCopyWindowInfo(
    $.kCGWindowListOptionOnScreenOnly | $.kCGWindowListExcludeDesktopElements, 0));
const owners = [];
for (let i = 0; i < list.count; i++) {
    const window = list.objectAtIndex(i);
    owners.push(ObjC.unwrap(window.objectForKey("kCGWindowOwnerName")) + "\t" +
        ObjC.unwrap(window.objectForKey("kCGWindowLayer")));
}
owners.join("\n")"#;

async fn on_screen_windows() -> Result<Vec<OnScreenWindow>> {
    let output = tokio::time::timeout(
        LOOK_DEADLINE,
        killed_on_drop("osascript")
            .args(["-l", "JavaScript", "-e", WINDOW_OWNERS_SCRIPT])
            .output(),
    )
    .await
    .context("listing on-screen windows took too long")?
    .context("failed to run osascript")?;
    anyhow::ensure!(
        output.status.success(),
        "osascript exited {}",
        output.status
    );
    windows_from(&String::from_utf8_lossy(&output.stdout))
}

/// One window a line, as `owner<TAB>layer`, blank lines dropped.
///
/// # Errors
/// Returns an error naming a line that has no tab or whose layer is not a number.
fn windows_from(listing: &str) -> Result<Vec<OnScreenWindow>> {
    listing
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let (owner, layer) = line
                .rsplit_once('\t')
                .with_context(|| format!("a window line has no layer: {line:?}"))?;
            let layer = layer
                .trim()
                .parse()
                .with_context(|| format!("a window's layer is not a number: {line:?}"))?;
            Ok(OnScreenWindow {
                owner: owner.trim().to_string(),
                layer,
            })
        })
        .collect()
}

async fn app_running() -> Result<bool> {
    let output = tokio::time::timeout(
        LOOK_DEADLINE,
        killed_on_drop("ps").args(["-axo", "comm="]).output(),
    )
    .await
    .context("listing processes took too long")?
    .context("failed to run ps")?;
    anyhow::ensure!(output.status.success(), "ps exited {}", output.status);
    Ok(the_app_is_among(&String::from_utf8_lossy(&output.stdout)))
}

/// Whether `executables`, one path a line as `ps -axo comm=` prints them, include the 1Password
/// desktop app itself. Not its helpers, its crash handler, or the Safari extension, which is also
/// an executable named `1Password`.
fn the_app_is_among(executables: &str) -> bool {
    executables.lines().any(|executable| {
        executable
            .trim_end()
            .ends_with("/1Password.app/Contents/MacOS/1Password")
    })
}

#[cfg(test)]
mod tests {

    use super::*;

    /// `ps -axo comm=` lines measured 2026-10-17 with the app open.
    const PROCESSES_WITH_THE_APP: &str = "\
2BUA8C4S2C.com.1password.browser-helper
/Applications/1Password for Safari.app/Contents/PlugIns/1Password.appex/Contents/MacOS/1Password
/Applications/1Password.app/Contents/MacOS/1Password
/Applications/1Password.app/Contents/Frameworks/1Password Helper.app/Contents/MacOS/1Password Helper
/Applications/1Password.app/Contents/MacOS/1Password-Crash-Handler
";

    #[test]
    fn the_app_is_found_among_its_helpers_and_extension() {
        assert!(the_app_is_among(PROCESSES_WITH_THE_APP));
    }

    #[test]
    fn the_safari_extension_and_helpers_alone_are_not_the_app() {
        let without_the_app: String = PROCESSES_WITH_THE_APP
            .lines()
            .filter(|line| *line != "/Applications/1Password.app/Contents/MacOS/1Password")
            .map(|line| format!("{line}\n"))
            .collect();

        assert!(!the_app_is_among(&without_the_app));
    }

    /// Reads the real window list, so it needs a macOS login session: run it by hand with
    /// `cargo nextest run -p config --run-ignored only`.
    #[cfg(target_os = "macos")]
    #[tokio::test]
    #[ignore = "needs a macOS login session"]
    async fn the_window_list_names_each_owner_and_layer() {
        let windows = on_screen_windows().await.unwrap();

        assert!(
            windows.iter().any(|window| window.owner == "Window Server"),
            "{windows:?}"
        );
        assert!(
            app_running().await.unwrap(),
            "the 1Password app is not running"
        );
    }

    #[test]
    fn windows_are_read_one_a_line_as_owner_and_layer() {
        assert_eq!(
            windows_from("Control Center\t25\n1Password\t101\n\n").unwrap(),
            vec![
                OnScreenWindow {
                    owner: "Control Center".to_string(),
                    layer: 25
                },
                OnScreenWindow {
                    owner: "1Password".to_string(),
                    layer: 101
                },
            ]
        );
    }

    #[test]
    fn a_window_line_without_a_layer_is_an_error() {
        assert!(windows_from("Control Center\n").is_err());
    }

    #[test]
    fn the_looks_combine_into_what_the_screen_showed() {
        assert_eq!(
            screen_from(
                Ok(vec![OnScreenWindow {
                    owner: "UserNotificationCenter".to_string(),
                    layer: 8
                }]),
                Ok(true)
            ),
            ScreenAtDeadline::MacosAlert { windows: 1 }
        );
        assert_eq!(
            screen_from(Ok(Vec::new()), Ok(false)),
            ScreenAtDeadline::AppNotRunning
        );
    }

    #[test]
    fn a_look_that_failed_makes_the_screen_unreadable_and_says_why() {
        let screen = screen_from(Err(anyhow::anyhow!("osascript exited 1")), Ok(true));

        assert_eq!(
            screen,
            ScreenAtDeadline::Unreadable {
                why: "osascript exited 1".to_string()
            }
        );
    }
}
