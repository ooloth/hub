use std::ffi::{CString, OsStr};
use std::io;
use std::os::fd::OwnedFd;
use std::os::unix::ffi::OsStrExt;

use tokio::net::unix::pipe;

use crate::disclaim_function;
use crate::errno::check;
use crate::file_actions::FileActions;
use crate::running_child::RunningChild;
use crate::spawn_attributes::SpawnAttributes;
use crate::unreaped::Unreaped;

/// Starts `program` with `args`, its stdin reading nothing and its stdout and stderr piped back.
///
/// The child is its own responsible process, so macOS judges what it may reach by its own
/// signature rather than by the signature of the program that started it, and it receives no
/// descriptor of ours except those pipes. It inherits our environment, as it is when `spawn`
/// runs, and our working directory.
///
/// Each pipe is created and then marked close-on-exec, because macOS has no call that does both
/// at once. A program another thread starts in between, the ordinary way, could inherit an end of
/// one: a read end would let it read what the child writes, and a write end would keep the read
/// from ending until the caller's deadline. A child started through this crate cannot, because it
/// closes every descriptor it is not given.
///
/// Must be called within a tokio runtime, which watches the pipes.
///
/// # Errors
/// Returns why the child could not be started, such as `NotFound` when `program` is not on the
/// `PATH`, or `Unsupported` when macOS lacks the function that disclaims responsibility.
#[allow(unsafe_code)]
pub fn spawn(program: impl AsRef<OsStr>, args: &[impl AsRef<OsStr>]) -> io::Result<RunningChild> {
    let attributes = SpawnAttributes::disclaiming(disclaim_function::lookup()?)?;
    let (stdout_read, stdout_write) = io::pipe()?;
    let (stderr_read, stderr_write) = io::pipe()?;
    let actions = FileActions::stdio(stdout_write, stderr_write)?;

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
    // The child has its copies of the write ends; closing ours lets each read end when it exits.
    drop(actions);
    Ok(RunningChild::new(
        child,
        pipe::Receiver::from_owned_fd(OwnedFd::from(stdout_read))?,
        pipe::Receiver::from_owned_fd(OwnedFd::from(stderr_read))?,
    ))
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
