use std::io;
use std::os::unix::process::ExitStatusExt;
use std::process::ExitStatus;

/// A child this crate started and has not yet reaped.
///
/// Only an unreaped pid is safe to signal: once reaped, the system may hand the same number to an
/// unrelated process. So reaping consumes this value, and code that signals after reaping does
/// not compile. Dropping it unreaped kills the child and reaps it.
///
/// This holds only while nothing else in the process reaps children it did not start, by calling
/// `waitpid(-1, …)` or by setting `SIGCHLD` to be ignored. Nothing in hub does either.
#[derive(Debug)]
pub(crate) struct Unreaped {
    pid: libc::pid_t,
}

impl Unreaped {
    pub(crate) fn new(pid: libc::pid_t) -> Self {
        assert!(pid > 0, "posix_spawnp reported success with pid {pid}");
        Self { pid }
    }

    pub(crate) const fn pid(&self) -> libc::pid_t {
        self.pid
    }

    /// The pid, given up without killing the child, for a moment when it may no longer be ours.
    const fn release(self) -> libc::pid_t {
        let pid = self.pid;
        std::mem::forget(self);
        pid
    }

    /// The child's exit status if it has exited, reaping it, or the child back if it has not.
    #[allow(unsafe_code)]
    pub(crate) fn try_reap(self) -> Result<ExitStatus, Self> {
        let mut status: libc::c_int = 0;
        // SAFETY: `status` is a local the call writes; WNOHANG makes it return at once.
        let reaped = unsafe { libc::waitpid(self.pid, &raw mut status, libc::WNOHANG) };
        match reaped {
            0 => Err(self),
            -1 => {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(libc::EINTR) {
                    return Err(self);
                }
                // Something else has reaped the child, so the pid may already name another
                // process: give it up before panicking, or the unwind would drop it and kill that
                // process.
                let pid = self.release();
                unreachable!("waitpid({pid}) failed: {error}; only this value reaps that pid");
            }
            other if other == self.pid => {
                // Reaped, so there is nothing left to kill, and the pid may already be reused.
                let _ = self.release();
                Ok(ExitStatus::from_raw(status))
            }
            other => {
                let pid = self.release();
                unreachable!("waitpid({pid}) returned pid {other}, which it was not asked for");
            }
        }
    }
}

impl Drop for Unreaped {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        let pid = self.pid;
        // SAFETY: the pid is still ours: every path that reaps it, or may have lost it, releases
        // the value first, so this drop never runs for it.
        let _ = unsafe { libc::kill(pid, libc::SIGKILL) };
        // Reaping blocks until the child has gone, which after SIGKILL is immediate, but a drop
        // can run on an async worker, so it happens on a thread of its own when one can start.
        let reaper = std::thread::Builder::new()
            .name(format!("reap-{pid}"))
            .spawn(move || reap_inline(pid));
        if reaper.is_err() {
            reap_inline(pid);
        }
    }
}

/// Waits for `pid`, already sent `SIGKILL`, to exit, and reaps it.
#[allow(unsafe_code)]
fn reap_inline(pid: libc::pid_t) {
    let mut status: libc::c_int = 0;
    // Bounded so a signal storm cannot keep the caller here forever.
    for _ in 0..1000 {
        // SAFETY: `status` is a local the call writes, and the caller is the only reaper left for
        // this pid.
        let reaped = unsafe { libc::waitpid(pid, &raw mut status, 0) };
        let interrupted =
            reaped == -1 && io::Error::last_os_error().raw_os_error() == Some(libc::EINTR);
        if !interrupted {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Whether `pid` is still in the process table, as a running process or a zombie.
    #[allow(unsafe_code)]
    fn exists(pid: libc::pid_t) -> bool {
        // SAFETY: signal 0 only asks whether the pid exists; it delivers nothing.
        unsafe { libc::kill(pid, 0) == 0 }
    }

    #[test]
    #[allow(unsafe_code)]
    #[allow(clippy::zombie_processes)] // reap_inline is the wait under test
    fn a_killed_child_reaped_inline_leaves_the_process_table() {
        let child = std::process::Command::new("/bin/sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let pid = libc::pid_t::try_from(child.id()).unwrap();
        // SAFETY: the child is ours and unreaped, so the pid names it.
        assert_eq!(unsafe { libc::kill(pid, libc::SIGKILL) }, 0);

        reap_inline(pid);

        assert!(!exists(pid), "pid {pid} is still in the process table");
    }
}
