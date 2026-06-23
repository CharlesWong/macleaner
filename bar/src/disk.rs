//! Boot-disk space via `statvfs(3)` (querying `$HOME` reports the writable Data
//! volume on macOS).

use std::ffi::CString;
use std::io;
use std::mem::MaybeUninit;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

const GIB: u64 = 1024 * 1024 * 1024;

fn statvfs_of(path: &Path) -> io::Result<libc::statvfs> {
    let cpath = CString::new(path.as_os_str().as_bytes())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    // SAFETY: valid C string + owned zeroed storage; return code checked before read.
    unsafe {
        let mut buf = MaybeUninit::<libc::statvfs>::zeroed();
        if libc::statvfs(cpath.as_ptr(), buf.as_mut_ptr()) != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(buf.assume_init())
    }
}

/// Bytes available to a non-root user on the volume holding `path`.
pub fn free_bytes(path: &Path) -> io::Result<u64> {
    let s = statvfs_of(path)?;
    Ok(s.f_bavail as u64 * s.f_frsize as u64)
}

/// Total bytes of the volume holding `path`.
pub fn total_bytes(path: &Path) -> io::Result<u64> {
    let s = statvfs_of(path)?;
    Ok(s.f_blocks as u64 * s.f_frsize as u64)
}

pub fn free_gb(path: &Path) -> io::Result<u64> {
    Ok(free_bytes(path)? / GIB)
}

/// Total volume size in whole gibibytes (at least 1).
pub fn total_gb(path: &Path) -> io::Result<u64> {
    Ok((total_bytes(path)? / GIB).max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_gb_positive_ac6() {
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or("/".into());
        assert!(free_gb(&home).unwrap() > 0);
    }

    #[test]
    fn total_ge_free() {
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or("/".into());
        assert!(total_gb(&home).unwrap() >= free_gb(&home).unwrap());
    }

    #[test]
    fn rejects_nul_path() {
        assert!(free_bytes(Path::new("/bad\0path")).is_err());
    }
}
