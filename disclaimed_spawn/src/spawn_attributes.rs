use std::io;

use crate::errno::check;

/// Close every descriptor the file actions do not set up, and restore the signal defaults and
/// mask given below.
const FLAGS: libc::c_int =
    libc::POSIX_SPAWN_CLOEXEC_DEFAULT | libc::POSIX_SPAWN_SETSIGDEF | libc::POSIX_SPAWN_SETSIGMASK;
const _: () = assert!(
    FLAGS <= libc::c_short::MAX as libc::c_int,
    "posix_spawnattr_setflags takes a short"
);

/// The attributes one `posix_spawnp` call is given, freed when dropped.
#[derive(Debug)]
pub(crate) struct SpawnAttributes(libc::posix_spawnattr_t);

impl SpawnAttributes {
    /// Attributes that disclaim responsibility, restore `SIGPIPE`'s default action and an empty
    /// signal mask in the child, and close every descriptor the file actions do not set up.
    ///
    /// Rust ignores `SIGPIPE` in its own processes, and an ignored signal stays ignored across
    /// exec, so without the reset the child would inherit a setting it never chose, as
    /// `std::process::Command` also prevents.
    ///
    /// # Errors
    /// Returns the errno any of the calls reported.
    #[allow(unsafe_code)]
    pub(crate) fn disclaiming(disclaim: crate::disclaim_function::DisclaimFn) -> io::Result<Self> {
        let mut attributes = Self(std::ptr::null_mut());
        // SAFETY: init writes a fresh attributes object into the pointer we own.
        check(unsafe { libc::posix_spawnattr_init(&raw mut attributes.0) })?;
        let mut defaults: libc::sigset_t = 0;
        let mut empty: libc::sigset_t = 0;
        // SAFETY: both sets are locals that outlive these calls, which read and write them.
        unsafe {
            check(libc::sigemptyset(&raw mut defaults))?;
            check(libc::sigaddset(&raw mut defaults, libc::SIGPIPE))?;
            check(libc::sigemptyset(&raw mut empty))?;
        }
        #[allow(clippy::cast_possible_truncation)] // FLAGS fits, as the const assertion checks
        let flags = FLAGS as libc::c_short;
        // SAFETY: `attributes.0` was initialised above, and each call reads the sets by
        // reference only for its own duration.
        unsafe {
            check(libc::posix_spawnattr_setsigdefault(
                &raw mut attributes.0,
                &raw const defaults,
            ))?;
            check(libc::posix_spawnattr_setsigmask(
                &raw mut attributes.0,
                &raw const empty,
            ))?;
            check(libc::posix_spawnattr_setflags(&raw mut attributes.0, flags))?;
            check(disclaim(&raw mut attributes.0, 1))?;
        }
        Ok(attributes)
    }

    pub(crate) const fn as_ptr(&self) -> *const libc::posix_spawnattr_t {
        &raw const self.0
    }
}

impl Drop for SpawnAttributes {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: a non-null pointer here was set by posix_spawnattr_init and is freed once.
            let _ = unsafe { libc::posix_spawnattr_destroy(&raw mut self.0) };
        }
    }
}
