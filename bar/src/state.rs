//! Reading macleaner's state (last-run timestamp, configured threshold) and
//! formatting it for display. Pure where possible.

use serde::Deserialize;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Home directory from `$HOME`.
pub fn home() -> io::Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "$HOME is not set"))
}

/// Current time as unix seconds.
pub fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// macleaner's last successful-run timestamp (unix seconds), if recorded.
pub fn read_last_run(home: &Path) -> Option<u64> {
    std::fs::read_to_string(home.join(".local/state/macleaner/last-run"))
        .ok()?
        .trim()
        .parse()
        .ok()
}

#[derive(Deserialize, Default)]
struct MinCfg {
    min_free_gb: Option<u64>,
}

/// macleaner's configured sweep threshold (GB), defaulting to 25 when the config
/// is absent or unparseable.
pub fn read_min_free_gb(home: &Path) -> u64 {
    std::fs::read_to_string(home.join(".config/macleaner/config.toml"))
        .ok()
        .and_then(|s| toml::from_str::<MinCfg>(&s).ok())
        .and_then(|c| c.min_free_gb)
        .unwrap_or(25)
}

/// Render an elapsed duration (seconds) as a coarse "ago" label.
pub fn ago_label(secs: u64) -> String {
    if secs < 120 {
        "just now".to_string()
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86_400 {
        format!("{}h ago", secs / 3600)
    } else {
        format!("{}d ago", secs / 86_400)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static N: AtomicU32 = AtomicU32::new(0);
    fn tmp_home() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "mbar-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn ago_label_ac2() {
        assert_eq!(ago_label(90), "just now");
        assert_eq!(ago_label(5 * 60), "5m ago");
        assert_eq!(ago_label(2 * 3600), "2h ago");
        assert_eq!(ago_label(3 * 86_400), "3d ago");
    }

    #[test]
    fn min_free_gb_default_and_value_ac5() {
        let home = tmp_home();
        assert_eq!(read_min_free_gb(&home), 25, "absent config → default 25");
        let cfg = home.join(".config/macleaner");
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::write(cfg.join("config.toml"), "min_free_gb = 10\n").unwrap();
        assert_eq!(read_min_free_gb(&home), 10);
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn last_run_roundtrip() {
        let home = tmp_home();
        assert_eq!(read_last_run(&home), None);
        let st = home.join(".local/state/macleaner");
        std::fs::create_dir_all(&st).unwrap();
        std::fs::write(st.join("last-run"), "1782152575\n").unwrap();
        assert_eq!(read_last_run(&home), Some(1782152575));
        std::fs::remove_dir_all(&home).ok();
    }
}
