use std::io;

/// An error for a call that returns 0 or an errno, as the `posix_spawn` family does.
pub(crate) fn check(code: libc::c_int) -> io::Result<()> {
    if code == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(code))
    }
}
