//! Cleaners: the units that actually reclaim space. Two shapes cover every
//! cleaner — an external-tool cleaner (`ToolCleaner`) and an age-gated directory
//! cleaner (`AgeDirCleaner`) — both bounded by the `safety` module.

use crate::config::Config;
use crate::report::CleanReport;
use crate::safety::{expand_home, is_kept_data, is_within_allowed_root, older_than};
use std::path::Path;
use std::process::Command;
use std::time::SystemTime;
use walkdir::WalkDir;

/// When a cleaner runs: every day, or only during a low-disk sweep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Daily,
    Sweep,
}

/// Runtime context handed to each cleaner.
pub struct Ctx {
    pub dry_run: bool,
    pub home: std::path::PathBuf,
    pub now: SystemTime,
}

/// A reclaimer. Implementations MUST respect `ctx.dry_run` (zero mutations) and
/// confine deletions to their own root via the `safety` helpers.
pub trait Cleaner {
    fn name(&self) -> &str;
    fn tier(&self) -> Tier;
    fn run(&self, ctx: &Ctx) -> CleanReport;
}

/// Recursive logical size of `path` (files only; never follows symlinks).
pub fn dir_size(path: &Path) -> u64 {
    WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .flatten()
        .filter_map(|e| e.metadata().ok())
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .sum()
}

/// Result of an age-prune over one directory.
#[derive(Debug, Default, Clone, Copy)]
pub struct PruneOutcome {
    pub bytes: u64,
    pub items: u64,
}

/// Prune the **top-level** entries of `root` whose modification time is older
/// than `age_days`, enforcing the safety invariants per entry: symlinks are
/// skipped (never followed or deleted-through); kept data (anything resolving
/// under `/Volumes`) is skipped; the entry must lie within `root` (allowlist);
/// names starting with any `exclude_prefixes` are skipped; and fresh entries
/// (within the age window) are never touched. In `dry_run` it computes the same
/// totals but removes nothing.
pub fn prune_dir_by_age(
    root: &Path,
    age_days: u64,
    exclude_prefixes: &[String],
    dry_run: bool,
    now: SystemTime,
) -> std::io::Result<PruneOutcome> {
    let mut out = PruneOutcome::default();
    if !root.exists() {
        return Ok(out);
    }
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();

        // symlink_metadata never follows the link.
        let meta = std::fs::symlink_metadata(&path)?;
        if meta.file_type().is_symlink() {
            continue; // never follow or delete through a symlink
        }
        if is_kept_data(&path) || !is_within_allowed_root(&path, root) {
            continue;
        }
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if exclude_prefixes
                .iter()
                .any(|p| name.starts_with(p.as_str()))
            {
                continue;
            }
        }
        let mtime = meta.modified().unwrap_or(now);
        if !older_than(mtime, now, age_days) {
            continue;
        }

        let sz = dir_size(&path);
        if !dry_run {
            if meta.is_dir() {
                std::fs::remove_dir_all(&path)?;
            } else {
                std::fs::remove_file(&path)?;
            }
        }
        out.bytes += sz;
        out.items += 1;
    }
    Ok(out)
}

/// Is `program` resolvable on `$PATH` (or an existing absolute path)?
pub fn tool_exists(program: &str) -> bool {
    if program.contains('/') {
        return Path::new(program).exists();
    }
    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths).any(|dir| {
                let c = dir.join(program);
                c.is_file()
            })
        })
        .unwrap_or(false)
}

/// True if a process with this exact name is running (`pgrep -x`).
fn process_running(name: &str) -> bool {
    Command::new("pgrep")
        .args(["-x", name])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// An external-tool cleaner (uv/npm/pnpm/brew/simctl/pip). Skipped cleanly when
/// the tool is absent, when a guard process is running, or in dry-run (a tool's
/// reclaim cannot be honestly estimated without running it).
pub struct ToolCleaner {
    pub name: String,
    pub tier: Tier,
    pub program: String,
    pub args: Vec<String>,
    /// Directory measured before/after to attribute reclaimed bytes (optional).
    pub measure: Option<String>,
    /// If a process with this name is running, skip (e.g. "uv" holds its lock).
    pub skip_if_process: Option<String>,
}

impl Cleaner for ToolCleaner {
    fn name(&self) -> &str {
        &self.name
    }
    fn tier(&self) -> Tier {
        self.tier
    }
    fn run(&self, ctx: &Ctx) -> CleanReport {
        if !tool_exists(&self.program) {
            return CleanReport::skipped(&self.name, "not installed");
        }
        if let Some(proc) = &self.skip_if_process {
            if process_running(proc) {
                return CleanReport::skipped(&self.name, &format!("{proc} running"));
            }
        }
        if ctx.dry_run {
            return CleanReport::skipped(&self.name, "dry-run (tool not estimated)");
        }
        let mdir = self.measure.as_ref().map(|d| expand_home(d, &ctx.home));
        let before = mdir.as_deref().map(dir_size).unwrap_or(0);
        match Command::new(&self.program).args(&self.args).output() {
            Ok(o) if o.status.success() => {
                let after = mdir.as_deref().map(dir_size).unwrap_or(0);
                CleanReport::freed(&self.name, before.saturating_sub(after), 1)
            }
            Ok(o) => {
                let err = String::from_utf8_lossy(&o.stderr);
                CleanReport::errored(
                    &self.name,
                    err.lines().next().unwrap_or("non-zero exit").trim(),
                )
            }
            Err(e) => CleanReport::errored(&self.name, &e.to_string()),
        }
    }
}

/// An age-gated directory cleaner (stale tmp, old logs, library caches, cargo
/// registry, trash). Each configured root is pruned via `prune_dir_by_age`.
pub struct AgeDirCleaner {
    pub name: String,
    pub tier: Tier,
    pub roots: Vec<String>,
    pub age_days: u64,
    pub exclude_prefixes: Vec<String>,
}

impl Cleaner for AgeDirCleaner {
    fn name(&self) -> &str {
        &self.name
    }
    fn tier(&self) -> Tier {
        self.tier
    }
    fn run(&self, ctx: &Ctx) -> CleanReport {
        let mut bytes = 0;
        let mut items = 0;
        for r in &self.roots {
            let root = expand_home(r, &ctx.home);
            match prune_dir_by_age(
                &root,
                self.age_days,
                &self.exclude_prefixes,
                ctx.dry_run,
                ctx.now,
            ) {
                Ok(o) => {
                    bytes += o.bytes;
                    items += o.items;
                }
                Err(e) => return CleanReport::errored(&self.name, &e.to_string()),
            }
        }
        CleanReport::freed(&self.name, bytes, items)
    }
}

/// Build the enabled cleaner set from config.
pub fn build_cleaners(cfg: &Config) -> Vec<Box<dyn Cleaner>> {
    let mut v: Vec<Box<dyn Cleaner>> = Vec::new();
    let d = &cfg.daily;

    if d.uv_cache {
        v.push(Box::new(ToolCleaner {
            name: "uv cache prune".into(),
            tier: Tier::Daily,
            program: "uv".into(),
            args: vec!["cache".into(), "prune".into()],
            measure: Some("~/.cache/uv".into()),
            skip_if_process: Some("uv".into()),
        }));
    }
    if d.npm_cache {
        v.push(Box::new(ToolCleaner {
            name: "npm cache verify".into(),
            tier: Tier::Daily,
            program: "npm".into(),
            args: vec!["cache".into(), "verify".into()],
            measure: Some("~/.npm".into()),
            skip_if_process: None,
        }));
    }
    if d.pnpm_store {
        v.push(Box::new(ToolCleaner {
            name: "pnpm store prune".into(),
            tier: Tier::Daily,
            program: "pnpm".into(),
            args: vec!["store".into(), "prune".into()],
            measure: None,
            skip_if_process: None,
        }));
    }
    if d.simulators {
        v.push(Box::new(ToolCleaner {
            name: "simctl prune".into(),
            tier: Tier::Daily,
            program: "xcrun".into(),
            args: vec!["simctl".into(), "delete".into(), "unavailable".into()],
            measure: None,
            skip_if_process: None,
        }));
    }
    if d.brew {
        v.push(Box::new(ToolCleaner {
            name: "brew cleanup".into(),
            tier: Tier::Daily,
            program: "brew".into(),
            args: vec!["cleanup".into()],
            measure: None,
            skip_if_process: None,
        }));
    }
    if d.stale_tmp.enabled {
        v.push(Box::new(AgeDirCleaner {
            name: "stale tmp".into(),
            tier: Tier::Daily,
            roots: d.stale_tmp.dirs.clone(),
            age_days: d.stale_tmp.age_days,
            exclude_prefixes: vec![],
        }));
    }
    if d.old_logs.enabled {
        v.push(Box::new(AgeDirCleaner {
            name: "old logs".into(),
            tier: Tier::Daily,
            roots: vec!["~/Library/Logs".into()],
            age_days: d.old_logs.age_days,
            exclude_prefixes: vec!["macleaner".into()], // never our own fresh logs
        }));
    }

    let s = &cfg.sweep;
    if s.library_caches.enabled {
        v.push(Box::new(AgeDirCleaner {
            name: "library caches".into(),
            tier: Tier::Sweep,
            roots: vec!["~/Library/Caches".into()],
            age_days: s.library_caches.age_days,
            exclude_prefixes: vec!["com.apple.".into()], // never Apple system caches
        }));
    }
    if s.pip_cache {
        v.push(Box::new(ToolCleaner {
            name: "pip cache purge".into(),
            tier: Tier::Sweep,
            program: "pip3".into(),
            args: vec!["cache".into(), "purge".into()],
            measure: Some("~/Library/Caches/pip".into()),
            skip_if_process: None,
        }));
    }
    if s.cargo_cache.enabled {
        v.push(Box::new(AgeDirCleaner {
            name: "cargo registry".into(),
            tier: Tier::Sweep,
            roots: vec![
                "~/.cargo/registry/cache".into(),
                "~/.cargo/registry/src".into(),
            ],
            age_days: s.cargo_cache.age_days,
            exclude_prefixes: vec![],
        }));
    }
    if s.trash.enabled {
        v.push(Box::new(AgeDirCleaner {
            name: "trash".into(),
            tier: Tier::Sweep,
            roots: vec!["~/.Trash".into()],
            age_days: s.trash.age_days,
            exclude_prefixes: vec![],
        }));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::path::PathBuf;
    use std::time::Duration;

    static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

    fn tmp_root() -> PathBuf {
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let p = std::env::temp_dir().join(format!("macleaner-test-{}-{n}", std::process::id()));
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn write_aged(path: &Path, contents: &[u8], days_old: u64) {
        fs::write(path, contents).unwrap();
        let when = SystemTime::now() - Duration::from_secs(days_old * 86_400 + 3600);
        File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(when)
            .unwrap();
    }

    #[test]
    fn prune_selects_only_old_ac2() {
        let root = tmp_root();
        let old = root.join("old.bin");
        let fresh = root.join("fresh.bin");
        write_aged(&old, &[0u8; 2048], 10);
        write_aged(&fresh, &[0u8; 4096], 1);

        let out = prune_dir_by_age(&root, 7, &[], false, SystemTime::now()).unwrap();
        assert_eq!(out.items, 1, "exactly one file should be old enough");
        assert_eq!(out.bytes, 2048);
        assert!(!old.exists(), "old file removed");
        assert!(fresh.exists(), "fresh file untouched");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn dry_run_reports_but_deletes_nothing_ac3() {
        let root = tmp_root();
        let old = root.join("old.bin");
        let fresh = root.join("fresh.bin");
        write_aged(&old, &[0u8; 2048], 10);
        write_aged(&fresh, &[0u8; 4096], 1);

        let out = prune_dir_by_age(&root, 7, &[], true, SystemTime::now()).unwrap();
        assert_eq!(out.items, 1);
        assert_eq!(out.bytes, 2048, "dry-run reports identical bytes");
        assert!(old.exists(), "dry-run deletes nothing");
        assert!(fresh.exists());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn does_not_descend_through_symlink_ac5() {
        use std::os::unix::fs::symlink;
        let root = tmp_root();
        let outside = tmp_root();
        let victim = outside.join("precious.bin");
        write_aged(&victim, &[7u8; 1024], 100); // very old, would match if followed
        let link = root.join("link_out");
        symlink(&outside, &link).unwrap();

        let out = prune_dir_by_age(&root, 7, &[], false, SystemTime::now()).unwrap();
        assert_eq!(out.items, 0, "symlink entry skipped, nothing pruned");
        assert!(victim.exists(), "file behind the symlink is untouched");
        assert!(link.exists(), "the symlink itself is not deleted-through");
        fs::remove_dir_all(&root).ok();
        fs::remove_dir_all(&outside).ok();
    }

    #[test]
    fn exclude_prefixes_protects_entries() {
        let root = tmp_root();
        let keep = root.join("com.apple.Safari");
        let drop = root.join("com.random.App");
        fs::create_dir_all(&keep).unwrap();
        fs::create_dir_all(&drop).unwrap();
        write_aged(&keep.join("f"), &[0u8; 100], 30);
        write_aged(&drop.join("f"), &[0u8; 100], 30);
        let excl = vec!["com.apple.".to_string()];
        let out = prune_dir_by_age(&root, 0, &excl, false, SystemTime::now()).unwrap();
        assert!(keep.exists(), "apple-prefixed cache protected");
        assert!(!drop.exists(), "non-excluded cache pruned");
        assert!(out.items >= 1);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn build_cleaners_has_daily_and_sweep() {
        let cfg = Config::default();
        let cleaners = build_cleaners(&cfg);
        assert!(cleaners.iter().any(|c| c.tier() == Tier::Daily));
        assert!(cleaners.iter().any(|c| c.tier() == Tier::Sweep));
        // uv cleaner must guard on the uv process.
        assert!(cleaners.iter().any(|c| c.name() == "uv cache prune"));
    }

    #[test]
    fn tool_exists_finds_sh_not_bogus() {
        assert!(tool_exists("/bin/sh"));
        assert!(!tool_exists("definitely-not-a-real-tool-xyz"));
    }
}
