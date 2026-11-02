//! Starts a program that macOS holds responsible for itself, rather than for the process that
//! started it. See `README.md` for why hub needs this and decision 027 for the choice.

#![warn(clippy::undocumented_unsafe_blocks)]

#[cfg(target_os = "macos")]
mod disclaim_function;
mod running_child;
mod spawn;
#[cfg(target_os = "macos")]
mod spawn_attributes;
#[cfg(target_os = "macos")]
mod unreaped;

pub use running_child::RunningChild;
pub use spawn::spawn;
