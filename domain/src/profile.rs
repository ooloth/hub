//! Which set of hub's own state a process reads and writes.

use std::path::{Path, PathBuf};

use anyhow::{bail, Result};

/// Which set of hub's own state a process reads and writes.
///
/// The set is closed, and hub owns both directories. An unrecognised
/// `HUB_PROFILE` is therefore a typo rather than a request for a third profile,
/// and [`Profile::parse`] rejects it naming both valid values.
///
/// Holding variants rather than a validated string is also what makes the
/// segment joined in [`Profile::dir`] a literal, so no externally-supplied text
/// reaches a path join and traversal is impossible rather than validated
/// against.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Profile {
    /// What the installed hub uses, and what an unset `HUB_PROFILE` means.
    #[default]
    Default,
    /// What every `just` recipe that runs hub from source uses.
    Dev,
}

impl Profile {
    /// The name of this profile, as it appears in `HUB_PROFILE` and in a path.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Dev => "dev",
        }
    }

    /// The directory holding this profile's state, under `home`.
    #[must_use]
    pub fn dir(self, home: &Path) -> PathBuf {
        home.join(".hub").join(self.as_str())
    }

    /// Parses the value of `HUB_PROFILE`.
    ///
    /// # Errors
    /// Returns an error naming both valid values when `raw` is neither.
    pub fn parse(raw: &str) -> Result<Self> {
        match raw {
            "default" => Ok(Self::Default),
            "dev" => Ok(Self::Dev),
            other => bail!(
                "unknown HUB_PROFILE {other:?}: expected \"{}\" or \"{}\"",
                Self::Default.as_str(),
                Self::Dev.as_str()
            ),
        }
    }
}

impl std::fmt::Display for Profile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn parse_accepts_the_default_profile() {
        assert_eq!(Profile::parse("default").unwrap(), Profile::Default);
    }

    #[test]
    fn parse_accepts_the_dev_profile() {
        assert_eq!(Profile::parse("dev").unwrap(), Profile::Dev);
    }

    #[test]
    fn an_unset_profile_means_default() {
        assert_eq!(Profile::default(), Profile::Default);
    }

    #[test]
    fn parse_rejection_names_both_valid_profiles() {
        let err = Profile::parse("dv").unwrap_err().to_string();
        assert!(err.contains("dv"), "error must quote what was given: {err}");
        assert!(err.contains("default"), "error must name default: {err}");
        assert!(err.contains("dev"), "error must name dev: {err}");
    }

    #[test]
    fn dir_places_the_default_profile_under_dot_hub() {
        assert_eq!(
            Profile::Default.dir(Path::new("/home/x")),
            PathBuf::from("/home/x/.hub/default")
        );
    }

    #[test]
    fn dir_places_the_dev_profile_under_dot_hub() {
        assert_eq!(
            Profile::Dev.dir(Path::new("/home/x")),
            PathBuf::from("/home/x/.hub/dev")
        );
    }

    #[test]
    fn dir_never_returns_the_home_directory_itself() {
        let home = Path::new("/home/x");
        assert_ne!(Profile::Default.dir(home), home);
        assert_ne!(Profile::Dev.dir(home), home);
    }

    proptest! {
        /// The set is closed: anything that is not one of the two names is a
        /// typo, whatever it looks like. Generated rather than enumerated
        /// because "every other string" is the only unbounded claim here.
        #[test]
        fn parse_rejects_every_name_outside_the_closed_set(
            raw in "\\PC{0,64}".prop_filter(
                "the two valid names are accepted, not rejected",
                |s| s != "default" && s != "dev",
            )
        ) {
            prop_assert!(
                Profile::parse(&raw).is_err(),
                "expected {raw:?} to be rejected"
            );
        }

        /// A parsed profile round-trips through the name it was parsed from,
        /// so `HUB_PROFILE` and the directory on disk cannot disagree.
        #[test]
        fn a_parsed_profile_round_trips_through_its_name(
            raw in prop::sample::select(vec!["default", "dev"])
        ) {
            prop_assert_eq!(Profile::parse(raw).unwrap().as_str(), raw);
        }
    }
}
