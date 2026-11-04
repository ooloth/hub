//! How hub started an `op` process: whether macOS holds hub responsible for it or `op` itself.
use std::collections::BTreeSet;
use std::fmt;

/// How one `op` process was started.
///
/// macOS judges what a process may reach by the process it holds responsible for it. An `op`
/// started the ordinary way answers to whatever is responsible for hub. A disclaimed one answers
/// for itself, which 1Password requires of the daemon's `op` on macOS 27 and which costs one macOS
/// dialog per `op` process on macOS 26 (decision 027, addenda of 2026-11-01 and 2026-11-03).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum OpSpawn {
    /// Started the ordinary way, so it answers to whatever is responsible for hub.
    Inherited,
    /// Started with responsibility disclaimed, so it answers for itself. On Linux, which holds no
    /// process responsible for another, this is an ordinary child (decision 031).
    Disclaimed,
}

impl OpSpawn {
    /// The word the log uses.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Inherited => "inherited",
            Self::Disclaimed => "disclaimed",
        }
    }
}

impl fmt::Display for OpSpawn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Every way of starting `op` that answered for one set of credentials. Empty when every
/// credential was a plain value, so no `op` was started.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpSpawnsUsed(BTreeSet<OpSpawn>);

impl OpSpawnsUsed {
    /// `spawn` answered for one more credential.
    pub fn add(&mut self, spawn: OpSpawn) {
        let _ = self.0.insert(spawn);
    }
}

impl fmt::Display for OpSpawnsUsed {
    /// `none`, or the spawns' words joined by commas, `inherited` first.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.is_empty() {
            return f.write_str("none");
        }
        let words: Vec<&str> = self.0.iter().map(|spawn| spawn.as_str()).collect();
        f.write_str(&words.join(","))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_spawn_reads_as_none() {
        assert_eq!(OpSpawnsUsed::default().to_string(), "none");
    }

    #[test]
    fn both_spawns_read_inherited_first_however_they_were_added() {
        let mut used = OpSpawnsUsed::default();
        used.add(OpSpawn::Disclaimed);
        used.add(OpSpawn::Inherited);
        used.add(OpSpawn::Disclaimed);

        assert_eq!(used.to_string(), "inherited,disclaimed");
    }
}
