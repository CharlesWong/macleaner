//! Safety primitives shared by every cleaner.
//!
//! This module is deliberately pure and small: a daemon that deletes files on a
//! schedule lives or dies by these checks. Nothing here touches the network and
//! every deletion path in the crate is expected to pass `is_within_allowed_root`
//! and `is_kept_data` before it removes anything.

use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime};

/// macOS external volumes mount here; anything that resolves under it is kept
/// data (e.g. the huggingface/whisper/puppeteer caches relocated off the boot
/// disk) and must never be deleted by this tool.
const VOLUMES_ROOT: &str = "/Volumes";

/// Human-readable byte formatting (1024-based). `format_bytes(1536) == "1.5 KB"`.
pub fn format_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if n < 1024 {
        return format!("{n} B");
    }
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    format!("{v:.1} {}", UNITS[i])
}

/// Resolve `.`/`..` textually WITHOUT touching the filesystem. Symlinks are not
/// expanded here on purpose — that is what makes this safe to call on paths that
/// may not exist (the runtime symlink defence is `is_kept_data` + a non-following
/// directory walk).
pub fn normalize_lexical(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in p.components() {
        match comp {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// True iff `path` lexically resolves to a location inside `root` AND is not
/// under `/Volumes`. This is the allowlist gate: a cleaner bound to `root` may
/// only act on paths this returns true for.
pub fn is_within_allowed_root(path: &Path, root: &Path) -> bool {
    let np = normalize_lexical(path);
    let nr = normalize_lexical(root);
    np.starts_with(&nr) && !np.starts_with(VOLUMES_ROOT)
}

/// True if the path is (or resolves through a symlink to) kept data on an
/// external volume. Canonicalizes when possible so a symlink such as
/// `~/.cache/huggingface -> /Volumes/External/caches/huggingface` is caught;
/// falls back to the lexical check when the path cannot be canonicalized.
pub fn is_kept_data(path: &Path) -> bool {
    if let Ok(real) = std::fs::canonicalize(path) {
        if real.starts_with(VOLUMES_ROOT) {
            return true;
        }
    }
    normalize_lexical(path).starts_with(VOLUMES_ROOT)
}

/// True if `mtime` is at least `days` old relative to `now`. A modification time
/// in the future is treated as NOT old (fail-safe: never delete a fresh file).
pub fn older_than(mtime: SystemTime, now: SystemTime, days: u64) -> bool {
    match now.duration_since(mtime) {
        Ok(age) => age >= Duration::from_secs(days * 86_400),
        Err(_) => false,
    }
}

/// Expand a leading `~` against `home`. Other `~user` forms are left untouched.
pub fn expand_home(spec: &str, home: &Path) -> PathBuf {
    if spec == "~" {
        return home.to_path_buf();
    }
    if let Some(rest) = spec.strip_prefix("~/") {
        return home.join(rest);
    }
    PathBuf::from(spec)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_bytes_ac1() {
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1024 * 1024), "1.0 MB");
        assert_eq!(format_bytes(3 * 1024 * 1024 * 1024), "3.0 GB");
    }

    #[test]
    fn within_root_accepts_child() {
        let root = Path::new("/Users/cw/.cache/uv");
        assert!(is_within_allowed_root(
            Path::new("/Users/cw/.cache/uv/wheels/foo.whl"),
            root
        ));
        assert!(is_within_allowed_root(root, root));
    }

    #[test]
    fn within_root_rejects_escape_and_volumes_ac4() {
        let root = Path::new("/Users/cw/.cache/uv");
        // escapes the root via ..
        assert!(!is_within_allowed_root(
            Path::new("/Users/cw/.cache/uv/../../.ssh/id_rsa"),
            root
        ));
        // a sibling outside the root
        assert!(!is_within_allowed_root(Path::new("/Users/cw/.ssh"), root));
        // anything under /Volumes is rejected even if it were the root
        let vroot = Path::new("/Volumes/External/caches");
        assert!(!is_within_allowed_root(
            Path::new("/Volumes/External/caches/huggingface/x"),
            vroot
        ));
    }

    #[test]
    fn kept_data_detects_volumes_lexically() {
        assert!(is_kept_data(Path::new("/Volumes/External/caches/whisper")));
        assert!(!is_kept_data(Path::new("/Users/cw/.cache/uv")));
    }

    #[test]
    fn older_than_ac2_boundary() {
        let now = SystemTime::now();
        let ten_days = now - Duration::from_secs(10 * 86_400);
        let one_day = now - Duration::from_secs(86_400);
        assert!(older_than(ten_days, now, 7));
        assert!(!older_than(one_day, now, 7));
        // future mtime is never "old"
        let future = now + Duration::from_secs(86_400);
        assert!(!older_than(future, now, 7));
    }

    #[test]
    fn expand_home_works() {
        let home = Path::new("/Users/cw");
        assert_eq!(expand_home("~", home), PathBuf::from("/Users/cw"));
        assert_eq!(
            expand_home("~/.gemini/tmp", home),
            PathBuf::from("/Users/cw/.gemini/tmp")
        );
        assert_eq!(expand_home("/tmp/x", home), PathBuf::from("/tmp/x"));
    }
}
