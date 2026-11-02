use std::io;
use std::process::Output;

/// A child started by [`crate::spawn`] that has not been waited for. Dropping it, or the future
/// [`RunningChild::output`] returns, before the child exits ends the child.
#[derive(Debug)]
pub struct RunningChild {
    #[cfg(target_os = "macos")]
    child: crate::unreaped::Unreaped,
    #[cfg(target_os = "macos")]
    stdout: tokio::net::unix::pipe::Receiver,
    #[cfg(target_os = "macos")]
    stderr: tokio::net::unix::pipe::Receiver,
    #[cfg(not(target_os = "macos"))]
    child: tokio::process::Child,
    pid: u32,
}

impl RunningChild {
    #[cfg(target_os = "macos")]
    pub(crate) fn new(
        child: crate::unreaped::Unreaped,
        stdout: tokio::net::unix::pipe::Receiver,
        stderr: tokio::net::unix::pipe::Receiver,
    ) -> Self {
        let pid = u32::try_from(child.pid()).unwrap_or_default();
        Self {
            child,
            stdout,
            stderr,
            pid,
        }
    }

    #[cfg(not(target_os = "macos"))]
    pub(crate) fn new(child: tokio::process::Child) -> Self {
        let pid = child.id().unwrap_or_default();
        Self { child, pid }
    }

    /// The child's process id.
    #[must_use]
    pub const fn pid(&self) -> u32 {
        self.pid
    }

    /// Waits for the child to exit and returns everything it wrote to stdout and stderr.
    ///
    /// Both pipes are read at once, so a child that fills one while we wait on the other cannot
    /// stall. Once both have ended, the child is checked every few milliseconds until it has
    /// exited. That wait has no limit of its own: a caller that needs one, as hub's credential
    /// deadline does, drops the future, which ends the child.
    ///
    /// # Errors
    /// Returns an error when reading the child's output fails.
    #[cfg(target_os = "macos")]
    pub async fn output(self) -> io::Result<Output> {
        use tokio::io::AsyncReadExt;

        let Self {
            mut child,
            mut stdout,
            mut stderr,
            ..
        } = self;
        let mut out = Vec::new();
        let mut err = Vec::new();
        let _ = tokio::try_join!(stdout.read_to_end(&mut out), stderr.read_to_end(&mut err))?;
        let status = loop {
            match child.try_reap() {
                Ok(status) => break status,
                Err(still_running) => {
                    child = still_running;
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                }
            }
        };
        Ok(Output {
            status,
            stdout: out,
            stderr: err,
        })
    }

    /// Waits for the child to exit and returns everything it wrote to stdout and stderr.
    ///
    /// # Errors
    /// Returns an error when reading the child's output fails.
    #[cfg(not(target_os = "macos"))]
    pub async fn output(self) -> io::Result<Output> {
        self.child.wait_with_output().await
    }
}
