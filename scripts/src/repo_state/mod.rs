//! Checks over the repository's own state, run by `just test`. See Decision 025.

/// Every workspace member has a README.
mod crate_readmes;
/// No superseded decision is listed as a design to build.
mod decision_status;
/// `domain/` reads no ambient state.
mod domain_purity;
/// Every workspace member inherits the workspace lints.
mod lint_inheritance;
/// Every snapshot file is produced by a test.
mod snapshot_coverage;
/// The workspace's members, as cargo sees them.
mod workspace;
