//! One daemon per profile: a lock held for the life of the process.
use std::fs::{File, OpenOptions, TryLockError};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// The lock that makes this process the profile's only daemon. Dropping it releases the lock,
/// and so does the process exiting, however it exits.
#[derive(Debug)]
pub(crate) struct InstanceLock {
    _file: File,
}

/// What trying to become the profile's daemon found.
#[derive(Debug)]
pub(crate) enum Acquired {
    /// This process holds the lock.
    Held(InstanceLock),
    /// Another daemon holds it. `pid` is what it wrote, or `None` if it has not written yet.
    HeldElsewhere { path: PathBuf, pid: Option<u32> },
}

impl InstanceLock {
    /// Takes the lock at `path`, creating the file and its directory if needed, and writes this
    /// process's PID into it. Never waits: a lock held elsewhere is reported at once.
    ///
    /// # Errors
    /// Returns an error when the file cannot be created, opened, locked or written.
    pub(crate) fn acquire(path: &Path) -> Result<Acquired> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("failed to create {}", dir.display()))?;
        }
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .with_context(|| format!("failed to open the lock file {}", path.display()))?;

        match file.try_lock() {
            Ok(()) => {
                file.set_len(0)
                    .and_then(|()| write!(file, "{}", std::process::id()))
                    .and_then(|()| file.flush())
                    .with_context(|| {
                        format!("failed to write this daemon's PID to {}", path.display())
                    })?;
                Ok(Acquired::Held(Self { _file: file }))
            }
            Err(TryLockError::WouldBlock) => {
                let mut contents = String::new();
                let pid = file
                    .read_to_string(&mut contents)
                    .ok()
                    .and_then(|_| contents.trim().parse().ok());
                Ok(Acquired::HeldElsewhere {
                    path: path.to_path_buf(),
                    pid,
                })
            }
            Err(TryLockError::Error(error)) => {
                Err(error).with_context(|| format!("failed to lock {}", path.display()))
            }
        }
    }
}

/// What a second daemon says before it exits.
pub(crate) fn refusal(profile: &str, path: &Path, pid: Option<u32>) -> String {
    let lock = path.display();
    pid.map_or_else(
        || {
            format!(
                "hub-daemon is already running for profile {profile} (pid unknown, lock {lock})"
            )
        },
        |pid| {
            format!(
                "hub-daemon is already running for profile {profile} (pid {pid}, lock {lock}). \
                 Stop it with: kill {pid}"
            )
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn held(acquired: Acquired) -> InstanceLock {
        match acquired {
            Acquired::Held(lock) => lock,
            Acquired::HeldElsewhere { .. } => panic!("expected to hold the lock"),
        }
    }

    #[test]
    fn the_first_daemon_holds_the_lock_and_records_its_pid() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dev/daemon.lock");

        let _lock = held(InstanceLock::acquire(&path).unwrap());

        assert_eq!(
            std::fs::read_to_string(&path).unwrap().trim(),
            std::process::id().to_string()
        );
    }

    #[test]
    fn a_second_daemon_finds_the_first_and_its_pid() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("daemon.lock");
        let _first = held(InstanceLock::acquire(&path).unwrap());

        let second = InstanceLock::acquire(&path).unwrap();

        match second {
            Acquired::HeldElsewhere { path: found, pid } => {
                assert_eq!(found, path);
                assert_eq!(pid, Some(std::process::id()));
            }
            Acquired::Held(_) => panic!("two daemons held the same profile's lock"),
        }
    }

    #[test]
    fn the_lock_is_free_again_once_its_holder_is_gone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("daemon.lock");
        drop(held(InstanceLock::acquire(&path).unwrap()));

        assert!(matches!(
            InstanceLock::acquire(&path).unwrap(),
            Acquired::Held(_)
        ));
    }

    #[test]
    fn each_profile_has_its_own_lock() {
        let dir = tempfile::tempdir().unwrap();
        let _dev = held(InstanceLock::acquire(&dir.path().join("dev/daemon.lock")).unwrap());

        let default = InstanceLock::acquire(&dir.path().join("default/daemon.lock")).unwrap();

        assert!(matches!(default, Acquired::Held(_)));
    }

    #[test]
    fn a_holder_that_has_not_written_its_pid_reads_as_unknown() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("daemon.lock");
        let holder = File::create(&path).unwrap();
        holder.try_lock().unwrap();

        let second = InstanceLock::acquire(&path).unwrap();

        assert!(matches!(second, Acquired::HeldElsewhere { pid: None, .. }));
    }

    #[test]
    fn the_refusal_names_the_profile_pid_and_lock_and_how_to_stop_it() {
        let message = refusal(
            "dev",
            Path::new("/home/me/.hub/dev/daemon.lock"),
            Some(4242),
        );

        assert!(message.contains("profile dev"), "{message}");
        assert!(message.contains("pid 4242"), "{message}");
        assert!(
            message.contains("/home/me/.hub/dev/daemon.lock"),
            "{message}"
        );
        assert!(message.contains("kill 4242"), "{message}");
    }

    #[test]
    fn without_a_pid_the_refusal_says_so_and_offers_no_kill() {
        let message = refusal("dev", Path::new("/l"), None);

        assert!(message.contains("pid unknown"), "{message}");
        assert!(!message.contains("kill"), "{message}");
    }
}
