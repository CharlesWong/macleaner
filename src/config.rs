//! Configuration: a TOML file at `~/.config/macleaner/config.toml`.
//!
//! Every field has a default (via `serde(default)`), so a partial or missing
//! file still yields a complete, conservative configuration.

use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};

/// Resolve the user's home directory from `$HOME`.
pub fn home_dir() -> io::Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "$HOME is not set"))
}

/// Path to the config file under `home`.
pub fn config_path(home: &Path) -> PathBuf {
    home.join(".config/macleaner/config.toml")
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Config {
    /// Master switch. When false, `run` does nothing.
    pub enabled: bool,
    /// Sweep-tier cleaners only run when boot-disk free space is below this.
    pub min_free_gb: u64,
    /// Minimum hours between successful runs (once-per-day guard).
    pub min_interval_hours: u64,
    /// launchd daily schedule.
    pub schedule_hour: u32,
    pub schedule_minute: u32,
    pub daily: DailyCfg,
    pub sweep: SweepCfg,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct DailyCfg {
    pub uv_cache: bool,
    pub npm_cache: bool,
    pub pnpm_store: bool,
    pub simulators: bool,
    pub brew: bool,
    pub stale_tmp: StaleTmpCfg,
    pub old_logs: AgeDir,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct SweepCfg {
    pub library_caches: AgeDir,
    pub pip_cache: bool,
    pub cargo_cache: AgeDir,
    pub trash: AgeDir,
}

/// An age-gated directory cleaner toggle.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct AgeDir {
    pub enabled: bool,
    pub age_days: u64,
}

/// Stale-temp cleaner: a set of directories whose stale *contents* are pruned.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct StaleTmpCfg {
    pub enabled: bool,
    pub age_days: u64,
    /// Directories (tilde-expanded) whose entries older than `age_days` are removed.
    pub dirs: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: true,
            min_free_gb: 25,
            min_interval_hours: 20,
            schedule_hour: 3,
            schedule_minute: 0,
            daily: DailyCfg::default(),
            sweep: SweepCfg::default(),
        }
    }
}

impl Default for DailyCfg {
    fn default() -> Self {
        Self {
            uv_cache: true,
            npm_cache: true,
            pnpm_store: true,
            simulators: true,
            brew: true,
            stale_tmp: StaleTmpCfg::default(),
            old_logs: AgeDir { enabled: true, age_days: 30 },
        }
    }
}

impl Default for SweepCfg {
    fn default() -> Self {
        Self {
            // Aggressive tier — enabled, but only fires below min_free_gb.
            library_caches: AgeDir { enabled: true, age_days: 14 },
            pip_cache: true,
            cargo_cache: AgeDir { enabled: true, age_days: 14 },
            trash: AgeDir { enabled: true, age_days: 30 },
        }
    }
}

impl Default for AgeDir {
    fn default() -> Self {
        Self { enabled: true, age_days: 14 }
    }
}

impl Default for StaleTmpCfg {
    fn default() -> Self {
        Self {
            enabled: true,
            age_days: 7,
            dirs: vec!["~/.gemini/tmp".to_string()],
        }
    }
}

impl Config {
    /// Load config from `home`, falling back to defaults when the file is absent.
    pub fn load(home: &Path) -> anyhow::Result<Self> {
        let path = config_path(home);
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(&path)?;
        Ok(toml::from_str(&text)?)
    }

    /// Serialize to TOML.
    pub fn to_toml(&self) -> anyhow::Result<String> {
        Ok(toml::to_string_pretty(self)?)
    }

    /// Write a default config file if none exists. Returns the path and whether
    /// it was created.
    pub fn ensure_file(home: &Path) -> anyhow::Result<(PathBuf, bool)> {
        let path = config_path(home);
        if path.exists() {
            return Ok((path, false));
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, Self::default().to_toml()?)?;
        Ok((path, true))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_conservative_and_complete() {
        let c = Config::default();
        assert!(c.enabled);
        assert_eq!(c.min_free_gb, 25);
        assert_eq!(c.min_interval_hours, 20);
        assert_eq!(c.schedule_hour, 3);
        assert_eq!(c.daily.stale_tmp.age_days, 7);
        assert_eq!(c.daily.old_logs.age_days, 30);
        assert_eq!(c.sweep.trash.age_days, 30);
        assert_eq!(c.sweep.library_caches.age_days, 14);
    }

    #[test]
    fn roundtrips_through_toml() {
        let c = Config::default();
        let text = c.to_toml().unwrap();
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(back.min_free_gb, c.min_free_gb);
        assert_eq!(back.daily.stale_tmp.dirs, c.daily.stale_tmp.dirs);
    }

    #[test]
    fn partial_toml_fills_defaults() {
        // Only one field set; everything else must come from defaults.
        let back: Config = toml::from_str("min_free_gb = 10\n").unwrap();
        assert_eq!(back.min_free_gb, 10);
        assert!(back.enabled);
        assert_eq!(back.daily.old_logs.age_days, 30);
    }
}
