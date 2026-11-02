use std::io;
use std::process::Output;

use tokio::io::AsyncReadExt;
use tokio::net::unix::pipe;

use crate::unreaped::Unreaped;

/// A child started by [`crate::spawn`] that has not been waited for. Dropping it, or the future
/// [`RunningChild::output`] returns, before the child exits ends the child.
#[derive(Debug)]
pub struct RunningChild {
    child: Unreaped,
    stdout: pipe::Receiver,
    stderr: pipe::Receiver,
}

impl RunningChild {
    pub(crate) const fn new(
        child: Unreaped,
        stdout: pipe::Receiver,
        stderr: pipe::Receiver,
    ) -> Self {
        Self {
            child,
            stdout,
            stderr,
        }
    }

    /// The child's process id.
    #[must_use]
    pub const fn pid(&self) -> u32 {
        // Lossless: `Unreaped` holds only a pid above 0.
        self.child.pid().cast_unsigned()
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
    pub async fn output(self) -> io::Result<Output> {
        let Self {
            mut child,
            mut stdout,
            mut stderr,
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
}
