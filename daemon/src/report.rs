//! Per-cleaner and per-run reporting.

use crate::safety::format_bytes;

/// Outcome of a single cleaner.
#[derive(Debug, Clone)]
pub struct CleanReport {
    pub name: String,
    /// Bytes reclaimed (or, in dry-run, bytes that WOULD be reclaimed).
    pub bytes: u64,
    /// Number of items (files/dirs) removed or that would be removed.
    pub items: u64,
    /// Set when the cleaner was skipped (tool absent, locked, disabled).
    pub skipped: Option<String>,
    /// Set when the cleaner errored (does not abort the run).
    pub error: Option<String>,
}

impl CleanReport {
    pub fn freed(name: &str, bytes: u64, items: u64) -> Self {
        Self { name: name.to_string(), bytes, items, skipped: None, error: None }
    }
    pub fn skipped(name: &str, why: &str) -> Self {
        Self { name: name.to_string(), bytes: 0, items: 0, skipped: Some(why.to_string()), error: None }
    }
    pub fn errored(name: &str, err: &str) -> Self {
        Self { name: name.to_string(), bytes: 0, items: 0, skipped: None, error: Some(err.to_string()) }
    }
    /// One-line human summary for stdout/log.
    pub fn line(&self) -> String {
        if let Some(why) = &self.skipped {
            return format!("  · {:<22} skipped ({why})", self.name);
        }
        if let Some(err) = &self.error {
            return format!("  ✗ {:<22} error: {err}", self.name);
        }
        format!(
            "  ✓ {:<22} {:>9}  ({} item{})",
            self.name,
            format_bytes(self.bytes),
            self.items,
            if self.items == 1 { "" } else { "s" }
        )
    }
}

/// Aggregate of a whole run.
#[derive(Debug, Clone)]
pub struct RunReport {
    pub dry_run: bool,
    pub swept: bool,
    pub free_gb_before: u64,
    pub reports: Vec<CleanReport>,
}

impl RunReport {
    pub fn total_bytes(&self) -> u64 {
        self.reports.iter().map(|r| r.bytes).sum()
    }
    pub fn total_items(&self) -> u64 {
        self.reports.iter().map(|r| r.items).sum()
    }
    /// Multi-line human report.
    pub fn render(&self) -> String {
        let mut s = String::new();
        let mode = if self.dry_run { "DRY-RUN" } else { "RUN" };
        let tier = if self.swept { "daily + sweep" } else { "daily" };
        s.push_str(&format!(
            "macleaner {mode} — {tier} tier — {} GB free before\n",
            self.free_gb_before
        ));
        for r in &self.reports {
            s.push_str(&r.line());
            s.push('\n');
        }
        let verb = if self.dry_run { "would reclaim" } else { "reclaimed" };
        s.push_str(&format!(
            "  ── {verb} {} across {} item{}\n",
            format_bytes(self.total_bytes()),
            self.total_items(),
            if self.total_items() == 1 { "" } else { "s" }
        ));
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn totals_sum_across_reports() {
        let rr = RunReport {
            dry_run: true,
            swept: false,
            free_gb_before: 40,
            reports: vec![
                CleanReport::freed("uv", 1000, 2),
                CleanReport::skipped("brew", "not installed"),
                CleanReport::freed("logs", 500, 3),
                CleanReport::errored("trash", "boom"),
            ],
        };
        assert_eq!(rr.total_bytes(), 1500);
        assert_eq!(rr.total_items(), 5);
        let out = rr.render();
        assert!(out.contains("DRY-RUN"));
        assert!(out.contains("would reclaim"));
        assert!(out.contains("skipped (not installed)"));
        assert!(out.contains("error: boom"));
    }
}
