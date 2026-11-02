use std::ffi::CStr;
use std::io;
use std::os::fd::{AsRawFd, OwnedFd};

/// The attributes one `posix_spawnp` call is given, freed when dropped.
#[derive(Debug)]
pub(crate) struct SpawnAttributes(libc::posix_spawnattr_t);

#[allow(unsafe_code)]
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
    pub(crate) fn disclaiming(disclaim: crate::disclaim_function::DisclaimFn) -> io::Result<Self> {
        let mut attributes = Self(std::ptr::null_mut());
        // SAFETY: init writes a fresh attributes object into the pointer we own.
        check(unsafe { libc::posix_spawnattr_init(&raw mut attributes.0) })?;
        let mut defaults: libc::sigset_t = 0;
        let mut empty: libc::sigset_t = 0;
        // SAFETY: both sets are local values these calls only write to.
        unsafe {
            check(libc::sigemptyset(&raw mut defaults))?;
            check(libc::sigaddset(&raw mut defaults, libc::SIGPIPE))?;
            check(libc::sigemptyset(&raw mut empty))?;
        }
        let flags = libc::POSIX_SPAWN_CLOEXEC_DEFAULT
            | libc::POSIX_SPAWN_SETSIGDEF
            | libc::POSIX_SPAWN_SETSIGMASK;
        // SAFETY: `attributes.0` was initialised above, and each call reads the sets by
        // reference only for its own duration. The flags fit the c_short the call takes.
        unsafe {
            check(libc::posix_spawnattr_setsigdefault(
                &raw mut attributes.0,
                &raw const defaults,
            ))?;
            check(libc::posix_spawnattr_setsigmask(
                &raw mut attributes.0,
                &raw const empty,
            ))?;
            check(libc::posix_spawnattr_setflags(
                &raw mut attributes.0,
                libc::c_short::try_from(flags)
                    .map_err(|_| io::Error::other("spawn flags overflow"))?,
            ))?;
            check(disclaim(&raw mut attributes.0, 1))?;
        }
        Ok(attributes)
    }

    pub(crate) const fn as_ptr(&self) -> *const libc::posix_spawnattr_t {
        &raw const self.0
    }
}

#[allow(unsafe_code)]
impl Drop for SpawnAttributes {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: a non-null pointer here was set by posix_spawnattr_init and is freed once.
            let _ = unsafe { libc::posix_spawnattr_destroy(&raw mut self.0) };
        }
    }
}

/// What `posix_spawnp` does to the child's descriptors before it runs, freed when dropped.
#[derive(Debug)]
pub(crate) struct FileActions(libc::posix_spawn_file_actions_t);

#[allow(unsafe_code)]
impl FileActions {
    /// Stdin from `/dev/null`, stdout and stderr onto the write ends of two pipes.
    ///
    /// # Errors
    /// Returns the errno any of the calls reported.
    pub(crate) fn stdio(stdout: &OwnedFd, stderr: &OwnedFd) -> io::Result<Self> {
        const DEV_NULL: &CStr = c"/dev/null";
        let mut actions = Self(std::ptr::null_mut());
        // SAFETY: init writes a fresh file-actions object into the pointer we own; each later
        // call records an action on it, copying the path, and the descriptors stay open until
        // the spawn that performs the actions has returned.
        unsafe {
            check(libc::posix_spawn_file_actions_init(&raw mut actions.0))?;
            check(libc::posix_spawn_file_actions_addopen(
                &raw mut actions.0,
                0,
                DEV_NULL.as_ptr(),
                libc::O_RDONLY,
                0,
            ))?;
            check(libc::posix_spawn_file_actions_adddup2(
                &raw mut actions.0,
                stdout.as_raw_fd(),
                1,
            ))?;
            check(libc::posix_spawn_file_actions_adddup2(
                &raw mut actions.0,
                stderr.as_raw_fd(),
                2,
            ))?;
        }
        Ok(actions)
    }

    pub(crate) const fn as_ptr(&self) -> *const libc::posix_spawn_file_actions_t {
        &raw const self.0
    }
}

#[allow(unsafe_code)]
impl Drop for FileActions {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: a non-null pointer here was set by posix_spawn_file_actions_init and is
            // freed once.
            let _ = unsafe { libc::posix_spawn_file_actions_destroy(&raw mut self.0) };
        }
    }
}

/// An error for a call that returns 0 or an errno, as the `posix_spawn` family does.
pub(crate) fn check(code: libc::c_int) -> io::Result<()> {
    if code == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(code))
    }
}
