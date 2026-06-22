//! Boot-disk free space via `statvfs(3)` (querying `$HOME` reports the writable
//! Data volume on macOS).

use std::ffi::CString;
use std::io;
use std::mem::MaybeUninit;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

pub fn free_bytes(path: &Path) -> io::Result<u64> {
    let cpath = CString::new(path.as_os_str().as_bytes())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    // SAFETY: valid C string + owned zeroed storage; return code checked before read.
    let stat = unsafe {
        let mut buf = MaybeUninit::<libc::statvfs>::zeroed();
        if libc::statvfs(cpath.as_ptr(), buf.as_mut_ptr()) != 0 {
            return Err(io::Error::last_os_error());
        }
        buf.assume_init()
    };
    Ok(stat.f_bavail as u64 * stat.f_frsize as u64)
}

pub fn free_gb(path: &Path) -> io::Result<u64> {
    Ok(free_bytes(path)? / (1024 * 1024 * 1024))
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
    fn rejects_nul_path() {
        assert!(free_bytes(Path::new("/bad\0path")).is_err());
    }
}
