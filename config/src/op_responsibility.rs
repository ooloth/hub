/// Which process macOS holds responsible for each `op` hub starts.
///
/// 1Password's CLI integration answers `op` only when macOS lets it reach the desktop app, and
/// macOS judges that by the responsible process. From a terminal that is the terminal app, which
/// works. Under launchd on macOS 27 it is `hub-daemon` itself, which the linker signs ad hoc, and
/// `op` then reports "No accounts configured". Decision 027's addendum of 2026-11-01 records the
/// measurements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpResponsibility {
    /// `op` answers to whatever is responsible for hub: the TUI's choice, unchanged.
    Inherited,
    /// `op` answers for itself, through `disclaimed_spawn`: the daemon's choice.
    Disclaimed,
}
