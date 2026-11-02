/// Which process macOS holds responsible for each `op` hub starts. The daemon needs its `op` to
/// answer for itself to reach 1Password under launchd; `disclaimed_spawn/README.md` explains why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpResponsibility {
    /// `op` answers to whatever is responsible for hub: the TUI's choice, unchanged.
    Inherited,
    /// `op` answers for itself, through `disclaimed_spawn`: the daemon's choice.
    Disclaimed,
}
