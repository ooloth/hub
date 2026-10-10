//! Running `hub-daemon` under launchd. See daemon/README.md
mod command;
mod definition;
mod launch_agent;
mod plan;

pub(crate) use command::{run, DaemonCommand};
#[cfg(test)]
pub(crate) use launch_agent::DAEMON_TOOLS;
