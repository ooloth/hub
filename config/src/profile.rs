//! Reads `HUB_PROFILE` from the environment.
//!
//! This is the only place hub reads that variable. Both `hub-tui` and
//! `hub-daemon` resolve a [`Profile`] here at startup and pass it down, so no
//! deeper caller reaches for the environment.

use anyhow::{Context, Result};
use domain::profile::Profile;

/// The environment variable naming which profile a process operates on.
pub const HUB_PROFILE: &str = "HUB_PROFILE";

/// Resolves the profile this process operates on.
///
/// An unset variable means [`Profile::Default`], which is what the installed
/// binaries get: `just` sets the variable for runs from source, a login shell
/// does not, and launchd never inherits one.
///
/// # Errors
/// Returns an error naming both valid profiles when the variable is set to
/// anything else, so a typo refuses rather than silently opening an empty
/// database.
pub fn from_env() -> Result<Profile> {
    let Ok(raw) = std::env::var(HUB_PROFILE) else {
        return Ok(Profile::default());
    };
    Profile::parse(&raw).with_context(|| format!("invalid {HUB_PROFILE}"))
}
