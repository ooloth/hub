use std::io;
use std::os::unix::process::ExitStatusExt;
use std::process::ExitStatus;

/// A child this crate started and has not yet reaped.
///
/// Only an unreaped pid is safe to signal: once reaped, the system may hand the same number to an
/// unrelated process. So reaping consumes this value, and code that signals after reaping does
/// not compile. Dropping it unreaped kills the child and reaps it off the async runtime.
#[derive(Debug)]
pub(crate) struct Unreaped {
    pid: libc::pid_t,
}

#[allow(unsafe_code)]
impl Unreaped {
    pub(crate) fn new(pid: libc::pid_t) -> Self {
        assert!(pid > 0, "posix_spawnp reported success with pid {pid}");
        Self { pid }
    }

    pub(crate) const fn pid(&self) -> libc::pid_t {
        self.pid
    }

    /// The child's exit status if it has exited, reaping it, or the child back if it has not.
    pub(crate) fn try_reap(self) -> Result<ExitStatus, Self> {
        let mut status: libc::c_int = 0;
        // SAFETY: `status` is a local the call writes; WNOHANG makes it return at once.
        let reaped = unsafe { libc::waitpid(self.pid, &raw mut status, libc::WNOHANG) };
        match reaped {
            0 => Err(self),
            -1 => {
                let error = io::Error::last_os_error();
                assert_eq!(
                    error.raw_os_error(),
                    Some(libc::EINTR),
                    "waitpid({}) failed: {error}; only this value reaps that pid",
                    self.pid
                );
                Err(self)
            }
            pid => {
                assert_eq!(pid, self.pid, "waitpid returned a pid it was not asked for");
                // Reaped, so there is nothing left to kill, and the pid may already be reused.
                std::mem::forget(self);
                Ok(ExitStatus::from_raw(status))
            }
        }
    }
}

#[allow(unsafe_code)]
impl Drop for Unreaped {
    fn drop(&mut self) {
        let pid = self.pid;
        // SAFETY: the pid is still ours, because only `try_reap` reaps it and it forgets `self`
        // when it does.
        let _ = unsafe { libc::kill(pid, libc::SIGKILL) };
        // Reaping blocks until the child has gone, which after SIGKILL is immediate, but a drop
        // can run on an async worker, so it happens on a thread of its own.
        let _ = std::thread::Builder::new()
            .name(format!("reap-{pid}"))
            .spawn(move || {
                let mut status: libc::c_int = 0;
                // Bounded so a signal storm cannot keep this thread alive forever.
                for _ in 0..1000 {
                    // SAFETY: as above; this thread is the only reaper left for this pid.
                    let reaped = unsafe { libc::waitpid(pid, &raw mut status, 0) };
                    let interrupted = reaped == -1
                        && io::Error::last_os_error().raw_os_error() == Some(libc::EINTR);
                    if !interrupted {
                        break;
                    }
                }
            });
    }
}
