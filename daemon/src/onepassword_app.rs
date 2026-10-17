//! Opening the 1Password desktop app. Opening it is the only thing hub ever does to it: nothing here
//! or anywhere else closes, quits or hides it.
use std::time::Duration;

use anyhow::{Context, Result};

/// How long `open` may take before the attempt to open the app counts as failed.
const OPEN_DEADLINE: Duration = Duration::from_secs(10);

/// The command that opens the app, in front, so someone returning sees it asking to be unlocked.
fn open_command() -> tokio::process::Command {
    let mut command = tokio::process::Command::new("open");
    let _ = command.args(["-a", "1Password"]).kill_on_drop(true);
    command
}

/// Opens the 1Password app, or says why it could not.
///
/// # Errors
/// Returns an error when `open` cannot be run, takes longer than [`OPEN_DEADLINE`], or exits
/// non-zero.
pub(crate) async fn open() -> Result<()> {
    let output = tokio::time::timeout(OPEN_DEADLINE, open_command().output())
        .await
        .context("opening the 1Password app took too long")?
        .context("failed to run open")?;
    anyhow::ensure!(
        output.status.success(),
        "open -a 1Password exited {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_runs_exactly_open_dash_a_1password() {
        let command = open_command();
        let std_command = command.as_std();

        assert_eq!(std_command.get_program(), "open");
        assert_eq!(
            std_command.get_args().collect::<Vec<_>>(),
            vec!["-a", "1Password"]
        );
    }
}
