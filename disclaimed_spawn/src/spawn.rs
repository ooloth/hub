use std::ffi::{CString, OsStr};
use std::io;
use std::os::fd::{FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;

use tokio::net::unix::pipe;

use crate::disclaim_function;
use crate::running_child::RunningChild;
use crate::spawn_attributes::{check, FileActions, SpawnAttributes};
use crate::unreaped::Unreaped;

/// Starts `program` with `args`, its stdin reading nothing and its stdout and stderr piped back.
///
/// The child is its own responsible process, so macOS judges what it may reach by its own
/// signature rather than by the signature of the program that started it, and it receives no
/// descriptor of ours except those pipes.
///
/// Must be called within a tokio runtime, which watches the pipes.
///
/// # Errors
/// Returns why the child could not be started, such as `NotFound` when `program` is not on the
/// `PATH`, or `Unsupported` when macOS lacks the function that disclaims responsibility.
#[allow(unsafe_code)]
pub fn spawn<S: AsRef<OsStr>>(program: S, args: &[S]) -> io::Result<RunningChild> {
    let attributes = SpawnAttributes::disclaiming(disclaim_function::lookup()?)?;
    let (stdout_read, stdout_write) = pipe_pair()?;
    let (stderr_read, stderr_write) = pipe_pair()?;
    let actions = FileActions::stdio(&stdout_write, &stderr_write)?;

    let program = c_string(program.as_ref().as_bytes())?;
    let arguments = std::iter::once(Ok(program.clone()))
        .chain(args.iter().map(|arg| c_string(arg.as_ref().as_bytes())))
        .collect::<io::Result<Vec<CString>>>()?;
    let environment = std::env::vars_os()
        .map(|(key, value)| c_string(&[key.as_bytes(), b"=", value.as_bytes()].concat()))
        .collect::<io::Result<Vec<CString>>>()?;
    let argv_ptrs = null_terminated(&arguments);
    let envp_ptrs = null_terminated(&environment);

    let mut pid: libc::pid_t = 0;
    // SAFETY: every pointer refers to a value that outlives the call: the attributes and
    // actions above, and NUL-terminated strings in null-terminated arrays built from owned
    // CStrings. posix_spawnp copies what it needs before returning.
    check(unsafe {
        libc::posix_spawnp(
            &raw mut pid,
            program.as_ptr(),
            actions.as_ptr(),
            attributes.as_ptr(),
            argv_ptrs.as_ptr().cast(),
            envp_ptrs.as_ptr().cast(),
        )
    })?;
    // From here an early return drops `child`, which kills the process.
    let child = Unreaped::new(pid);
    drop((stdout_write, stderr_write));
    Ok(RunningChild::new(
        child,
        pipe::Receiver::from_owned_fd(stdout_read)?,
        pipe::Receiver::from_owned_fd(stderr_read)?,
    ))
}

/// A pipe's read and write ends, both closed on exec, so a child some other code starts at the
/// same moment cannot hold a write end open and keep our read from ever ending.
#[allow(unsafe_code)]
fn pipe_pair() -> io::Result<(OwnedFd, OwnedFd)> {
    let mut fds = [0 as libc::c_int; 2];
    // SAFETY: `fds` has room for the two descriptors pipe writes.
    if unsafe { libc::pipe(fds.as_mut_ptr()) } == -1 {
        return Err(io::Error::last_os_error());
    }
    let [read, write] = fds;
    // SAFETY: pipe succeeded, so both are open descriptors nothing else owns.
    let ends = unsafe { (OwnedFd::from_raw_fd(read), OwnedFd::from_raw_fd(write)) };
    for fd in [read, write] {
        // SAFETY: `fd` is open and owned by `ends` for the length of this call.
        if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } == -1 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(ends)
}

fn c_string(bytes: &[u8]) -> io::Result<CString> {
    CString::new(bytes).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "a program, argument or variable holds a NUL byte",
        )
    })
}

fn null_terminated(strings: &[CString]) -> Vec<*const libc::c_char> {
    strings
        .iter()
        .map(|string| string.as_ptr())
        .chain(std::iter::once(std::ptr::null()))
        .collect()
}
