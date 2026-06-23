//! Side-effecting actions: drive the macleaner CLI, open the log, post
//! notifications. All external calls go through `Command` args (no shell), so
//! there is no shell-injection surface.

use std::path::{Path, PathBuf};
use std::process::Command;

pub fn macleaner_bin(home: &Path) -> PathBuf {
    home.join("bin/macleaner")
}
pub fn log_path(home: &Path) -> PathBuf {
    home.join("Library/Logs/macleaner/macleaner.log")
}

/// Run macleaner with `args`; return the summary line (the reclaim line) or a
/// short status string. Never panics.
fn run_macleaner(home: &Path, args: &[&str]) -> String {
    let bin = macleaner_bin(home);
    if !bin.exists() {
        return "macleaner not installed (~/bin/macleaner)".to_string();
    }
    match Command::new(&bin).args(args).output() {
        Ok(o) => {
            let out = String::from_utf8_lossy(&o.stdout);
            out.lines()
                .rev()
                .find(|l| l.contains("reclaim"))
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|| "done".to_string())
        }
        Err(e) => format!("error: {e}"),
    }
}

pub fn run_dry(home: &Path) -> String {
    run_macleaner(home, &["dry-run"])
}

/// Run macleaner and return its FULL stdout (for parsing the per-cleaner table).
/// Empty string if the binary is missing or errors.
pub fn run_capture(home: &Path, args: &[&str]) -> String {
    let bin = macleaner_bin(home);
    if !bin.exists() {
        return String::new();
    }
    Command::new(&bin)
        .args(args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

pub fn open_log(home: &Path) {
    let _ = Command::new("open").arg(log_path(home)).spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_under_home() {
        let home = Path::new("/Users/cw");
        assert_eq!(macleaner_bin(home), Path::new("/Users/cw/bin/macleaner"));
        assert!(log_path(home).ends_with("Library/Logs/macleaner/macleaner.log"));
    }
}
