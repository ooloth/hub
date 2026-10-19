//! Domain types and pure logic for hub.

/// CI failure signals from GitHub Actions.
pub mod ci;
/// One failed attempt by the daemon to obtain its credentials.
pub mod credential_attempt;
/// Why the daemon could not obtain its credentials from 1Password.
pub mod credential_failure;
/// The notification posted when the daemon has gone without credentials for a while.
pub mod credential_notification;
/// What the daemon's health record says about it.
pub mod daemon_health;
/// One refresh pass of the daemon, as it is logged and recorded.
pub mod daemon_pass;
/// GCP Cloud Logging query configuration and log entry types.
pub mod gcp;
/// The MCP servers an investigation may use, chosen per device.
pub mod investigation_mcp_servers;
/// Investigation prompts, assembled from parts that say where each came from.
pub mod investigation_prompt;
/// Deterministic tmux window names for the windows hub opens.
pub mod investigation_window;
/// GitHub and Linear issue types and label helpers.
pub mod issue;
/// The credential values a failure reason must never contain.
pub mod known_secrets;
/// Loki query configuration and log entry types.
pub mod loki;
/// Which pull requests have a notification showing, and what to change when the turns change.
pub mod notified;
/// Why a whole refresh pass failed, in a form safe to log and store.
pub mod pass_failure;
/// Pull request types and repository slug newtype.
pub mod pr;
/// The notification group a pull request's card is posted in, and reading groups back.
pub mod pr_group;
/// Which pull request something is about.
pub mod pr_id;
/// Whether a pull request is waiting on me, and the card that says so.
pub mod pr_turn;
/// Which set of hub's own state a process reads and writes.
pub mod profile;
/// What the screen showed when an `op read` reached its deadline.
pub mod screen_at_deadline;
/// Serde helpers for non-standard wire formats (e.g. duration as seconds).
pub mod serde_helpers;
/// Claude Code session transcript parsing.
pub mod session;
/// A source that failed during a refresh, and why, in a form safe to log and store.
pub mod source_failure;
/// Which sources a refresh heard from, and which failed.
pub mod source_outcomes;
/// Which pull requests one refresh found waiting on me.
pub mod turns_seen;
/// Text hub did not write.
pub mod untrusted;
/// Urgency ranking used across all signal types.
pub mod urgency;

// At the crate root because hub-private's workflows name it as `domain::duration_secs` in their
// `#[serde(with = ...)]` attributes. This repo's own types use `crate::serde_helpers::duration_secs`.
pub use serde_helpers::duration_secs;

pub use ci::*;
pub use gcp::*;
pub use investigation_mcp_servers::*;
pub use investigation_prompt::*;
pub use investigation_window::*;
pub use issue::*;
pub use loki::*;
pub use pr::*;
pub use profile::*;
pub use session::*;
pub use untrusted::*;
pub use urgency::*;
