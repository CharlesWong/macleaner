//! Orchestration: the once-per-day guard, free-space tier selection, fail-soft
//! execution of every cleaner, and logging.

use crate::cleaners::{build_cleaners, Ctx, Tier};
use crate::config::Config;
use crate::disk::free_gb;
use crate::report::RunReport;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub struct RunOpts {
    pub dry_run: bool,
    pub force: bool,
}

fn state_path(home: &Path) -> PathBuf {
    home.join(".local/state/macleaner/last-run")
}

pub fn log_path(home: &Path) -> PathBuf {
    home.join("Library/Logs/macleaner/macleaner.log")
}

/// Read the last successful-run timestamp, if any.
pub fn read_last_run(home: &Path) -> Option<SystemTime> {
    let secs: u64 = std::fs::read_to_string(state_path(home)).ok()?.trim().parse().ok()?;
    Some(UNIX_EPOCH + Duration::from_secs(secs))
}

pub fn write_last_run(home: &Path, now: SystemTime) -> std::io::Result<()> {
    let p = state_path(home);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let secs = now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    std::fs::write(p, secs.to_string())
}

/// The once-per-day guard. `--force` always runs; otherwise a run is allowed
/// only if at least `min_interval` has elapsed since the last run.
pub fn should_run(
    last: Option<SystemTime>,
    now: SystemTime,
    force: bool,
    min_interval: Duration,
) -> bool {
    if force {
        return true;
    }
    match last {
        None => true,
        Some(t) => now.duration_since(t).map(|d| d >= min_interval).unwrap_or(true),
    }
}

/// Execute a run. Returns `None` when nothing ran (disabled, or guarded by the
/// once-per-day rule); `Some(report)` otherwise.
pub fn execute(cfg: &Config, home: &Path, opts: &RunOpts) -> anyhow::Result<Option<RunReport>> {
    if !cfg.enabled {
        return Ok(None);
    }
    let now = SystemTime::now();
    // The guard applies to real runs only; a dry-run is always allowed so the
    // operator can inspect on demand.
    if !opts.dry_run
        && !should_run(
            read_last_run(home),
            now,
            opts.force,
            Duration::from_secs(cfg.min_interval_hours * 3600),
        )
    {
        return Ok(None);
    }

    let free_before = free_gb(home).unwrap_or(u64::MAX);
    let swept = free_before < cfg.min_free_gb;

    let ctx = Ctx { dry_run: opts.dry_run, home: home.to_path_buf(), now };
    let mut reports = Vec::new();
    for cleaner in build_cleaners(cfg) {
        if cleaner.tier() == Tier::Sweep && !swept {
            // Sweep tier only fires when the disk is filling up; record the
            // skip so the report shows it was considered, not silently dropped.
            reports.push(crate::report::CleanReport::skipped(
                cleaner.name(),
                "disk above threshold",
            ));
            continue;
        }
        reports.push(cleaner.run(&ctx));
    }

    let report = RunReport { dry_run: opts.dry_run, swept, free_gb_before: free_before, reports };

    // Persist the timestamp only for real runs that were not a total failure.
    if !opts.dry_run && report.reports.iter().any(|r| r.error.is_none()) {
        write_last_run(home, now)?;
    }
    append_log(home, &report).ok();
    Ok(Some(report))
}

fn append_log(home: &Path, report: &RunReport) -> std::io::Result<()> {
    let p = log_path(home);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(p)?;
    writeln!(f, "[{stamp}] {}", report.render().replace('\n', " | "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn once_per_day_guard_ac6() {
        let now = SystemTime::now();
        let interval = Duration::from_secs(20 * 3600);
        let one_hour_ago = now - Duration::from_secs(3600);
        let twentyfive_ago = now - Duration::from_secs(25 * 3600);

        assert!(!should_run(Some(one_hour_ago), now, false, interval));
        assert!(should_run(Some(twentyfive_ago), now, false, interval));
        assert!(should_run(Some(one_hour_ago), now, true, interval)); // --force
        assert!(should_run(None, now, false, interval)); // never run before
    }

    #[test]
    fn disabled_config_runs_nothing() {
        let cfg = Config { enabled: false, ..Config::default() };
        let home = std::env::temp_dir();
        let out = execute(&cfg, &home, &RunOpts { dry_run: true, force: true }).unwrap();
        assert!(out.is_none());
    }
}
