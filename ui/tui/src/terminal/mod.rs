//! The windows hub opens beside the TUI, in whichever of tmux or Rex encloses it.

mod detect;
mod rex;
mod tmux;
mod window;

pub(crate) use window::{claim, WindowClaim};
