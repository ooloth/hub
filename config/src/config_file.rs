use std::collections::HashMap;
use std::future::Future;
use std::time::Duration;

use anyhow::{Context, Result};
use domain::credential_failure::CredentialFailure;
use domain::known_secrets::KnownSecrets;
use domain::screen_at_deadline::{OnScreenWindow, ScreenAtDeadline};
use secrecy::Secret;

use crate::resolved::Config;
use crate::toml;

/// `hub.toml`, read and checked, with its credentials still as written: plain values or `op://`
/// references nobody has asked 1Password about yet.
///
/// Reading it can fail only on the file itself. Resolving it can fail only on a credential, so a
/// caller can try [`ConfigFile::resolve`] again without ever retrying a broken file.
#[derive(Debug, Clone)]
pub struct ConfigFile {
    credentials: toml::CredentialsToml,
    projects: crate::Projects,
    monitor: Option<toml::Monitor>,
    investigation_mcp_servers: domain::InvestigationMcpServers,
}

impl ConfigFile {
    /// Reads `hub.toml` from the working directory.
    ///
    /// # Errors
    /// Returns an error if `hub.toml` is missing or malformed, lacks a required credential, or
    /// gives a project a label hub refuses.
    pub fn read() -> Result<Self> {
        let hub_toml = toml::parse_file("hub.toml")?;
        validate_required(&hub_toml.credentials)?;
        let investigation_mcp_servers = hub_toml.investigation.mcp_servers()?;
        let projects = crate::Projects::resolve(hub_toml.project)
            .context("hub.toml has a [[project]] hub cannot use")?;
        Ok(Self {
            credentials: hub_toml.credentials,
            projects,
            monitor: hub_toml.monitor,
            investigation_mcp_servers,
        })
    }

    /// Resolves every credential, asking 1Password for each `op://` reference.
    ///
    /// # Errors
    /// Returns why the first credential that could not be resolved failed, redacted of every
    /// credential resolved before it.
    pub async fn resolve(&self) -> Result<Config, CredentialFailure> {
        let creds = &self.credentials;
        let mut so_far = ResolvedSoFar::default();
        let github_token = so_far.read(&creds.github_token).await?;
        let linear_token = match &creds.linear_token {
            Some(value) => Some(so_far.read(value).await?),
            None => None,
        };
        let loki_token = match &creds.loki_token {
            Some(value) => Some(so_far.read(value).await?),
            None => None,
        };
        let mut extra_credentials = HashMap::new();
        for (name, value) in &creds.extra {
            let _ = extra_credentials.insert(name.clone(), so_far.read(value).await?);
        }
        Ok(Config {
            github_token,
            github_username: creds.github_username.clone(),
            linear_token,
            loki_token,
            extra_credentials,
            projects: self.projects.clone(),
            monitor: self.monitor.clone(),
            investigation_mcp_servers: self.investigation_mcp_servers.clone(),
        })
    }
}

/// How long one `op read` may take before it is ended.
///
/// Longer than [`OP_PROMPT_LIMIT`], so it never cuts short a prompt `op` is still showing: past it,
/// `op` is not waiting on anyone, it has gone silent.
const OP_DEADLINE: Duration = Duration::from_secs(90);

/// How long `op` waits on an approval prompt nobody answers before giving up with "authorization
/// timeout": 60 seconds, measured three times with `op` 2.40.0 on 2026-10-17.
const OP_PROMPT_LIMIT: Duration = Duration::from_secs(60);

const _: () = assert!(
    OP_DEADLINE.as_secs() > OP_PROMPT_LIMIT.as_secs(),
    "the deadline would end prompts op is still showing"
);

/// How much longer a read may run when 1Password's Touch ID prompt is on screen at the deadline.
/// Longer than [`OP_PROMPT_LIMIT`], so `op` gives up on the prompt by itself first.
const TOUCH_ID_GRACE: Duration = Duration::from_secs(65);

const _: () = assert!(
    TOUCH_ID_GRACE.as_secs() > OP_PROMPT_LIMIT.as_secs(),
    "the extra wait would end a Touch ID prompt op is still showing"
);

/// The credentials one call to [`ConfigFile::resolve`] has resolved so far, which a later failure
/// in the same call must not repeat.
#[derive(Debug, Default)]
struct ResolvedSoFar {
    values: Vec<String>,
}

impl ResolvedSoFar {
    /// Resolves `value`, remembering it, or fails redacted of everything remembered before it.
    async fn read(&mut self, value: &str) -> Result<Secret<String>, CredentialFailure> {
        self.answered_within(
            value,
            OP_DEADLINE,
            read_credential(value),
            look_at_screen_and_app,
        )
        .await
    }

    /// What `read` gave for `reference`, or when it had not finished by `deadline`, a
    /// [`CredentialFailure::no_answer`] naming what `look` then found. When `look` finds 1Password's
    /// Touch ID prompt, `read` gets [`TOUCH_ID_GRACE`] longer instead, since `op` gives up on that
    /// prompt by itself. Dropping `read` ends the `op` it started.
    async fn answered_within<Look, Seen>(
        &mut self,
        reference: &str,
        deadline: Duration,
        read: impl Future<Output = Result<String>>,
        mut look: Look,
    ) -> Result<Secret<String>, CredentialFailure>
    where
        Look: FnMut() -> Seen,
        Seen: Future<Output = ScreenAtDeadline>,
    {
        let mut read = std::pin::pin!(read);
        let mut waited = deadline;
        let mut answered = tokio::time::timeout(deadline, &mut read).await;
        if answered.is_err() {
            let seen = look().await;
            if seen != ScreenAtDeadline::TouchIdPrompt {
                return Err(self.no_answer(reference, waited, &seen));
            }
            // op gives up on a Touch ID prompt by itself within a minute, so let it.
            waited += TOUCH_ID_GRACE;
            answered = tokio::time::timeout(TOUCH_ID_GRACE, &mut read).await;
        }
        match answered {
            Ok(Ok(resolved)) => {
                self.values.push(resolved.clone());
                Ok(Secret::new(resolved))
            }
            Ok(Err(error)) => Err(self.failed(&error)),
            Err(_elapsed) => Err(self.no_answer(reference, waited, &look().await)),
        }
    }

    fn no_answer(
        &self,
        reference: &str,
        waited: Duration,
        seen: &ScreenAtDeadline,
    ) -> CredentialFailure {
        CredentialFailure::no_answer(reference, waited, seen, &self.known_secrets())
    }

    fn failed(&self, error: &anyhow::Error) -> CredentialFailure {
        CredentialFailure::new(error, &self.known_secrets())
    }

    fn known_secrets(&self) -> KnownSecrets {
        KnownSecrets::new(self.values.clone())
    }
}

fn validate_required(creds: &toml::CredentialsToml) -> Result<()> {
    if creds.github_token.is_empty() {
        anyhow::bail!("credentials.github_token is required in hub.toml");
    }
    if creds.github_username.is_empty() {
        anyhow::bail!("credentials.github_username is required in hub.toml");
    }
    Ok(())
}

/// `value` itself, or for an `op://` reference, what `op read` returns for it.
async fn read_credential(value: &str) -> Result<String> {
    if !value.starts_with("op://") {
        return Ok(value.to_string());
    }
    let output = killed_on_drop("op")
        .args(["read", value])
        .output()
        .await
        .context("failed to run `op read` — is the 1Password CLI installed?")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "failed to resolve credential from 1Password: {}",
            stderr.trim()
        );
    }
    let resolved = String::from_utf8(output.stdout).context("op read returned non-UTF-8 output")?;
    Ok(resolved.trim().to_string())
}

/// How long each look at the screen or the process list may take.
const LOOK_DEADLINE: Duration = Duration::from_secs(5);

/// Who was not answering when an `op read` reached its deadline: whether a macOS alert is on screen,
/// and whether the 1Password app is running.
async fn look_at_screen_and_app() -> ScreenAtDeadline {
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

/// A command whose process is killed when the future running it is dropped.
///
/// A read gives up on `op` at its deadline by dropping its future. Without this, an `op` that never
/// answers would keep running after the attempt that gave up on it.
fn killed_on_drop(program: &str) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(program);
    let _ = command.kill_on_drop(true);
    command
}

#[cfg(test)]
mod tests {
    use domain::credential_failure::CredentialCause;
    use secrecy::ExposeSecret;

    use super::*;

    fn credentials(github_token: &str, github_username: &str) -> toml::CredentialsToml {
        toml::CredentialsToml {
            github_token: github_token.into(),
            github_username: github_username.into(),
            linear_token: None,
            loki_token: None,
            extra: HashMap::new(),
        }
    }

    #[tokio::test]
    async fn plain_credentials_resolve_without_asking_1password() {
        let mut creds = credentials("tok", "user");
        creds.linear_token = Some("lin".into());
        let _ = creds.extra.insert("media".into(), "key".into());
        let file = ConfigFile {
            credentials: creds,
            projects: crate::Projects::default(),
            monitor: None,
            investigation_mcp_servers: domain::InvestigationMcpServers::default(),
        };

        let config = file.resolve().await.unwrap();

        assert_eq!(config.github_token.expose_secret(), "tok");
        assert_eq!(config.github_username, "user");
        assert_eq!(
            config
                .linear_token
                .as_ref()
                .map(ExposeSecret::expose_secret),
            Some(&"lin".to_string())
        );
        assert_eq!(config.extra_credentials["media"].expose_secret(), "key");
    }

    #[tokio::test]
    async fn a_failure_never_repeats_a_credential_resolved_before_it() {
        let mut so_far = ResolvedSoFar::default();
        let _ = so_far.read("tok-4f9a").await.unwrap();

        let failure = so_far.failed(&anyhow::anyhow!("op echoed tok-4f9a back"));

        assert!(
            !failure.reason().as_str().contains("tok-4f9a"),
            "{}",
            failure.reason()
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_read_unfinished_at_the_deadline_names_who_the_screen_says_was_not_answering() {
        let mut so_far = ResolvedSoFar::default();
        let _ = so_far.read("tok-4f9a").await.unwrap();

        let failure = so_far
            .answered_within(
                "op://Scripts/GitHub/token",
                OP_DEADLINE,
                std::future::pending::<Result<String>>(),
                || async { ScreenAtDeadline::MacosAlert { windows: 1 } },
            )
            .await
            .unwrap_err();

        assert_eq!(failure.cause(), CredentialCause::MacosPromptWaitingForYou);
        assert!(
            failure.reason().as_str().starts_with(
                "1Password did not answer within 90s reading op://Scripts/GitHub/token; "
            ),
            "{}",
            failure.reason()
        );
    }

    #[tokio::test]
    async fn a_read_that_finishes_never_looks_at_the_screen() {
        let mut so_far = ResolvedSoFar::default();

        let read = so_far
            .answered_within(
                "op://x/y/z",
                OP_DEADLINE,
                async { Ok("value".to_string()) },
                || async { panic!("looked at the screen for a read that answered") },
            )
            .await;

        assert!(read.is_ok());
    }

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

    #[tokio::test(start_paused = true)]
    async fn a_touch_id_prompt_on_screen_at_the_deadline_lets_the_read_finish() {
        let mut so_far = ResolvedSoFar::default();
        let late = async {
            tokio::time::sleep(OP_DEADLINE + Duration::from_secs(30)).await;
            Ok("value".to_string())
        };

        let read = so_far
            .answered_within("op://x/y/z", OP_DEADLINE, late, || async {
                ScreenAtDeadline::TouchIdPrompt
            })
            .await;

        assert!(read.is_ok(), "{read:?}");
    }

    #[tokio::test(start_paused = true)]
    async fn a_touch_id_prompt_still_up_after_the_extra_wait_ends_the_read() {
        let mut so_far = ResolvedSoFar::default();

        let failure = so_far
            .answered_within(
                "op://x/y/z",
                OP_DEADLINE,
                std::future::pending::<Result<String>>(),
                || async { ScreenAtDeadline::TouchIdPrompt },
            )
            .await
            .unwrap_err();

        assert_eq!(failure.cause(), CredentialCause::TouchIdPromptUnanswered);
        assert_eq!(
            failure.reason().as_str(),
            "1Password did not answer within 155s reading op://x/y/z; 1Password's Touch ID prompt \
             is on screen"
        );
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

    #[tokio::test]
    async fn a_process_still_running_at_the_deadline_is_gone_straight_after_it() {
        let read = killed_on_drop("sleep").arg("30").output();

        let timed = tokio::time::timeout(Duration::from_millis(200), read).await;
        tokio::time::sleep(Duration::from_millis(200)).await;

        assert!(timed.is_err(), "sleep 30 finished within 200 ms");
        let children = std::process::Command::new("pgrep")
            .args(["-x", "sleep", "-P", &std::process::id().to_string()])
            .output()
            .unwrap();
        assert!(
            children.stdout.is_empty(),
            "a sleep child is still running: {}",
            String::from_utf8_lossy(&children.stdout)
        );
    }

    #[test]
    fn validate_required_passes_with_both_fields_set() {
        assert!(validate_required(&credentials("tok", "user")).is_ok());
    }

    #[test]
    fn validate_required_fails_on_empty_github_token() {
        let err = validate_required(&credentials("", "user")).unwrap_err();
        assert!(err.to_string().contains("github_token"));
    }

    #[test]
    fn validate_required_fails_on_empty_github_username() {
        let err = validate_required(&credentials("tok", "")).unwrap_err();
        assert!(err.to_string().contains("github_username"));
    }

    #[tokio::test]
    async fn a_plain_value_is_its_own_credential() {
        assert_eq!(read_credential("plain-value").await.unwrap(), "plain-value");
    }

    #[tokio::test]
    async fn an_empty_value_is_its_own_credential() {
        assert_eq!(read_credential("").await.unwrap(), "");
    }
}
