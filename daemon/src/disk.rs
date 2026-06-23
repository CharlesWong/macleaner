//! Boot-disk free-space query via `statvfs(3)`.

use std::ffi::CString;
use std::io;
use std::mem::MaybeUninit;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

/// Free bytes available to a non-root user on the filesystem containing `path`.
/// On macOS, querying `$HOME` reports the writable Data volume (the one that
/// fills up), not the read-only system volume.
pub fn free_bytes(path: &Path) -> io::Result<u64> {
    let cpath = CString::new(path.as_os_str().as_bytes())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    // SAFETY: `statvfs` writes into `buf`; we pass a valid C string and a
    // pointer to owned, correctly-sized, zeroed storage. We check the return
    // code before reading the struct.
    let stat = unsafe {
        let mut buf = MaybeUninit::<libc::statvfs>::zeroed();
        if libc::statvfs(cpath.as_ptr(), buf.as_mut_ptr()) != 0 {
            return Err(io::Error::last_os_error());
        }
        buf.assume_init()
    };
    Ok(stat.f_bavail as u64 * stat.f_frsize as u64)
}

/// Free space in whole gibibytes (rounded down) on the volume holding `path`.
pub fn free_gb(path: &Path) -> io::Result<u64> {
    Ok(free_bytes(path)? / (1024 * 1024 * 1024))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_bytes_on_root_is_positive() {
        // The filesystem under "/" always has some availability on a live host.
        let n = free_bytes(Path::new("/")).expect("statvfs / should succeed");
        assert!(n > 0, "expected some free space, got {n}");
    }

    #[test]
    fn free_bytes_rejects_nul_in_path() {
        assert!(free_bytes(Path::new("/bad\0path")).is_err());
    }
}
