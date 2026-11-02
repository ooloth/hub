use std::ffi::CStr;
use std::io::{self, PipeWriter};
use std::os::fd::AsRawFd;

use crate::errno::check;

/// What `posix_spawnp` does to the child's descriptors before it runs, and the pipe write ends it
/// hands the child. Owning them keeps them open for as long as the actions name them, and
/// dropping the actions frees them and closes the parent's copies, so the child's exit is what
/// ends the reads.
#[derive(Debug)]
pub(crate) struct FileActions {
    actions: libc::posix_spawn_file_actions_t,
    _stdout: PipeWriter,
    _stderr: PipeWriter,
}

impl FileActions {
    /// Stdin from `/dev/null`, stdout and stderr onto the write ends of two pipes.
    ///
    /// # Errors
    /// Returns the errno any of the calls reported.
    #[allow(unsafe_code)]
    pub(crate) fn stdio(stdout: PipeWriter, stderr: PipeWriter) -> io::Result<Self> {
        const DEV_NULL: &CStr = c"/dev/null";
        let (stdout_fd, stderr_fd) = (stdout.as_raw_fd(), stderr.as_raw_fd());
        let mut actions = Self {
            actions: std::ptr::null_mut(),
            _stdout: stdout,
            _stderr: stderr,
        };
        // SAFETY: init writes a fresh file-actions object into the pointer we own; each later
        // call records an action on it, copying the path. The descriptors it names are owned by
        // `actions`, so they stay open for as long as the actions exist.
        unsafe {
            check(libc::posix_spawn_file_actions_init(
                &raw mut actions.actions,
            ))?;
            check(libc::posix_spawn_file_actions_addopen(
                &raw mut actions.actions,
                0,
                DEV_NULL.as_ptr(),
                libc::O_RDONLY,
                0,
            ))?;
            check(libc::posix_spawn_file_actions_adddup2(
                &raw mut actions.actions,
                stdout_fd,
                1,
            ))?;
            check(libc::posix_spawn_file_actions_adddup2(
                &raw mut actions.actions,
                stderr_fd,
                2,
            ))?;
        }
        Ok(actions)
    }

    pub(crate) const fn as_ptr(&self) -> *const libc::posix_spawn_file_actions_t {
        &raw const self.actions
    }
}

impl Drop for FileActions {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        if !self.actions.is_null() {
            // SAFETY: a non-null pointer here was set by posix_spawn_file_actions_init and is
            // freed once.
            let _ = unsafe { libc::posix_spawn_file_actions_destroy(&raw mut self.actions) };
        }
    }
}
