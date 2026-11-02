use std::ffi::CStr;
use std::io;

/// The name of the libSystem function that marks a spawn as disclaiming responsibility.
pub(crate) const NAME: &CStr = c"responsibility_spawnattrs_setdisclaim";

/// `responsibility_spawnattrs_setdisclaim`: sets or clears the disclaim flag on spawn attributes,
/// returning 0 or an errno. Apple publishes no header for it; LLDB declares it this way in
/// `lldb/source/Host/macosx/objcxx/PosixSpawnResponsible.h`.
pub(crate) type DisclaimFn = unsafe extern "C" fn(
    attributes: *mut libc::posix_spawnattr_t,
    disclaim: libc::c_int,
) -> libc::c_int;

/// Finds the disclaim function in the libraries already loaded, as LLDB does, so a macOS without
/// it gives an error hub can record instead of a daemon that fails to load.
///
/// # Errors
/// Returns `Unsupported`, naming the function, when it is not there.
#[allow(unsafe_code)]
pub(crate) fn lookup() -> io::Result<DisclaimFn> {
    // SAFETY: `NAME` is a NUL-terminated string that outlives the call, and RTLD_DEFAULT asks
    // every image already loaded. dlsym reads nothing else of ours.
    let address = unsafe { libc::dlsym(libc::RTLD_DEFAULT, NAME.as_ptr()) };
    let found = (!address.is_null()).then(|| {
        // SAFETY: a non-null address dlsym returned for this name is that function, whose
        // signature `DisclaimFn` declares as LLDB does.
        unsafe { std::mem::transmute::<*mut libc::c_void, DisclaimFn>(address) }
    });
    from_lookup(found)
}

/// The disclaim function, given what looking it up in libSystem found.
///
/// # Errors
/// Returns `Unsupported`, naming the function, when the lookup found nothing.
pub(crate) fn from_lookup<F>(found: Option<F>) -> io::Result<F> {
    found.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::Unsupported,
            format!(
                "{} is not available on this macOS, so the child cannot answer for itself",
                NAME.to_string_lossy()
            ),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_found_function_is_returned() {
        assert_eq!(from_lookup(Some(7)).unwrap(), 7);
    }

    #[test]
    fn a_missing_function_is_unsupported_and_named() {
        let error = from_lookup::<u8>(None).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
        assert!(
            error.to_string().contains(&*NAME.to_string_lossy()),
            "{error}"
        );
    }

    #[test]
    fn this_macos_has_the_function() {
        assert!(lookup().is_ok());
    }
}
