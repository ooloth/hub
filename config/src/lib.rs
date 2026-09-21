//! Configuration loading for hub — reads `hub.toml`, resolves credentials, and
//! exposes a typed [`Config`] used by both the CLI and TUI.

/// Raw TOML types that mirror the structure of `hub.toml`.
pub mod toml;

/// Resolved, credential-injected configuration used at runtime.
pub mod resolved;

/// Reads `HUB_PROFILE` from the environment.
pub mod profile;

pub use resolved::Config;
