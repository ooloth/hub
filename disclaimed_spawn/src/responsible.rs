use std::ffi::CStr;
use std::io;

/// The name of the libSystem function that says which process macOS holds responsible for another.
const NAME: &CStr = c"responsibility_get_pid_responsible_for_pid";

/// `responsibility_get_pid_responsible_for_pid`: the pid macOS holds responsible for `pid`, or -1.
/// Apple publishes no header for it; it takes and returns an `int`.
type ResponsibleFn = unsafe extern "C" fn(pid: libc::pid_t) -> libc::pid_t;

/// The process macOS holds responsible for `pid`.
///
/// This exists so that tests can prove a spawn disclaims: a child of a disclaimed process answers
/// to that process, and a child of an ordinary one answers to whatever its parent answers to. The
/// function is looked up at runtime, as the disclaim function is, so a macOS without it gives
/// `Unsupported` rather than a binary that fails to load.
///
/// # Errors
/// Returns `Unsupported`, naming the function, when this macOS lacks it, and `NotFound` when macOS
/// knows no process `pid`.
#[allow(unsafe_code)]
pub fn responsible_for(pid: u32) -> io::Result<u32> {
    let asked = libc::pid_t::try_from(pid)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, format!("{pid} is not a pid")))?;
    // SAFETY: `NAME` is a NUL-terminated string that outlives the call, and RTLD_DEFAULT asks
    // every image already loaded. dlsym reads nothing else of ours.
    let address = unsafe { libc::dlsym(libc::RTLD_DEFAULT, NAME.as_ptr()) };
    if address.is_null() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!("{} is not available on this macOS", NAME.to_string_lossy()),
        ));
    }
    // SAFETY: a non-null address dlsym returned for this name is that function, which takes a pid
    // by value and returns one, touching no memory of ours.
    let answer = unsafe {
        let function = std::mem::transmute::<*mut libc::c_void, ResponsibleFn>(address);
        function(asked)
    };
    u32::try_from(answer).map_err(|_| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("macOS knows no process responsible for {pid}"),
        )
    })
}
