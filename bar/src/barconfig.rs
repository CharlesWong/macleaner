//! The menu-bar app's own tiny config: whether it wants to start at login.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct BarConfig {
    pub start_at_login: bool,
}

impl Default for BarConfig {
    fn default() -> Self {
        Self { start_at_login: true }
    }
}

pub fn config_path(home: &Path) -> PathBuf {
    home.join(".config/macleaner-bar/config.toml")
}

impl BarConfig {
    pub fn load(home: &Path) -> Self {
        std::fs::read_to_string(config_path(home))
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, home: &Path) -> anyhow::Result<()> {
        let p = config_path(home);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(p, toml::to_string_pretty(self)?)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static N: AtomicU32 = AtomicU32::new(0);

    #[test]
    fn defaults_to_start_at_login() {
        assert!(BarConfig::default().start_at_login);
    }

    #[test]
    fn roundtrips() {
        let home = std::env::temp_dir()
            .join(format!("mbar-cfg-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        std::fs::create_dir_all(&home).unwrap();
        let c = BarConfig { start_at_login: false };
        c.save(&home).unwrap();
        assert!(!BarConfig::load(&home).start_at_login);
        std::fs::remove_dir_all(&home).ok();
    }
}
