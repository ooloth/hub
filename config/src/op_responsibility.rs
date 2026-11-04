/// How the `op` processes one resolution starts may answer to macOS.
///
/// Either every `op` answers to whatever is responsible for hub, or each is started that way first
/// and read again through an `op` that answers for itself when 1Password refuses it.
/// `disclaimed_spawn/README.md` explains why the daemon needs the second.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpResponsibility {
    /// Every `op` answers to whatever is responsible for hub: the TUI's choice.
    Inherited,
    /// Every `op` is first started that way, and a credential 1Password refuses with "No accounts
    /// configured" is read again through an `op` that answers for itself: the daemon's choice
    /// (decision 027, addendum of 2026-11-03).
    InheritedUnlessRefused,
}
