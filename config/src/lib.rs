//! Configuration loading for hub — reads `hub.toml`, resolves credentials, and
//! exposes a typed [`Config`] used by both the CLI and TUI.

/// Raw TOML types that mirror the structure of `hub.toml`.
pub mod toml;

/// The `[investigation]` section: what every investigation on this device receives.
pub mod investigation;

/// `hub.toml` as read, before any credential is resolved.
pub mod config_file;

/// Projects with every label settled, as the rest of hub sees them.
pub mod project;

/// Resolved, credential-injected configuration used at runtime.
pub mod resolved;

/// Reads `HUB_PROFILE` from the environment.
pub mod profile;

pub use config_file::ConfigFile;
pub use project::{Project, Projects};
pub use resolved::Config;
