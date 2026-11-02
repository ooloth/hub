//! Starts a program that macOS holds responsible for itself, rather than for the process that
//! started it. See `README.md` for why hub needs this and decision 027 for the choice.
//!
//! The crate is empty on every other platform. hub builds there too (decision 031), but no other
//! platform holds a child responsible for its parent, so there is nothing to disclaim.

#![cfg(target_os = "macos")]
#![warn(clippy::undocumented_unsafe_blocks)]

mod disclaim_function;
mod running_child;
mod spawn;
mod spawn_attributes;
mod unreaped;

pub use running_child::RunningChild;
pub use spawn::spawn;
