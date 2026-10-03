//! Checks over the repository's own state, run by `just test`. See Decision 025.

/// Every workspace member has a README.
mod crate_readmes;
/// The workspace's members, as cargo sees them.
mod workspace;
