//! Starts a program that macOS holds responsible for itself, rather than for the process that
//! started it. See `README.md` for why hub needs this and decision 027 for the choice.

#![warn(clippy::undocumented_unsafe_blocks)]

#[cfg(not(target_os = "macos"))]
compile_error!("disclaimed_spawn runs only on macOS: hub has no other platform");

mod disclaim_function;
mod running_child;
mod spawn;
mod spawn_attributes;
mod unreaped;

pub use running_child::RunningChild;
pub use spawn::spawn;
