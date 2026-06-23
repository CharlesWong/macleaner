//! Pure logic for the WebView panel: the embedded HTML, the action protocol,
//! disk-usage math, and parsing macleaner's output into the panel's payloads.

use serde::Serialize;

/// The panel UI, rendered in a WKWebView.
pub const PANEL_HTML: &str = include_str!("panel.html");

/// Fallback boot-disk size for the ring readout (GB) when `statvfs` can't report
/// the real total. The real total is preferred (see `disk::total_gb`).
pub const TOTAL_GB: u64 = 256;

/// Messages the WebView can post to native.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Action {
    Ready,
    CleanNow,
    Preview,
    OpenLog,
    ToggleLogin,
    Done,
    Quit,
}

impl Action {
    pub fn parse(s: &str) -> Option<Action> {
        Some(match s {
            "ready" => Action::Ready,
            "cleanNow" => Action::CleanNow,
            "preview" => Action::Preview,
            "openLog" => Action::OpenLog,
            "toggleLogin" => Action::ToggleLogin,
            "done" => Action::Done,
            "quit" => Action::Quit,
            _ => return None,
        })
    }
}

/// Percent of `total_gb` used, given `free_gb`; clamped to 0..=100, and 0 when
/// `total_gb` is 0 (no divide-by-zero).
pub fn used_pct(free_gb: u64, total_gb: u64) -> u64 {
    if total_gb == 0 {
        return 0;
    }
    let used = total_gb.saturating_sub(free_gb) as f64;
    ((used / total_gb as f64) * 100.0).round().clamp(0.0, 100.0) as u64
}

/// 1024-based human bytes ("4.2 GB", "512 B"), matching macleaner's formatting.
pub fn format_bytes(n: u64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if n < 1024 {
        return format!("{n} B");
    }
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    format!("{v:.1} {}", U[i])
}

fn unit_mult(u: &str) -> Option<f64> {
    Some(match u {
        "B" => 1.0,
        "KB" => 1024.0,
        "MB" => 1024f64.powi(2),
        "GB" => 1024f64.powi(3),
        "TB" => 1024f64.powi(4),
        _ => return None,
    })
}

/// Parse a size from a whitespace-split string, accepting both the spaced form
/// ("4.2 GB") and a concatenated form ("4.2GB"); returns bytes (1024-based).
fn parse_size_field(s: &str) -> Option<u64> {
    // concatenated form: a single "<number><unit>" word like "4.2GB"
    for tok in s.split_whitespace() {
        if let Some(us) = tok.find(|c: char| c.is_ascii_alphabetic()) {
            if us > 0 {
                if let (Ok(v), Some(m)) = (
                    tok[..us].parse::<f64>(),
                    unit_mult(tok[us..].trim_end_matches(|c: char| !c.is_ascii_alphabetic())),
                ) {
                    return Some((v * m) as u64);
                }
            }
        }
    }
    // spaced: "<number>" then "<unit>"
    let toks: Vec<&str> = s.split_whitespace().collect();
    for w in toks.windows(2) {
        if let Ok(v) = w[0].parse::<f64>() {
            if let Some(m) = unit_mult(w[1].trim_matches(|c: char| !c.is_ascii_alphabetic())) {
                return Some((v * m) as u64);
            }
        }
    }
    None
}

/// Total reclaimed bytes from a macleaner summary line ("… reclaimed 4.2 GB …"
/// or "would reclaim 1.5 GB"). 0 if absent.
pub fn parse_reclaimed_bytes(output: &str) -> u64 {
    for line in output.lines() {
        if let Some(idx) = line.find("reclaim") {
            if let Some(b) = parse_size_field(&line[idx..]) {
                return b;
            }
        }
    }
    0
}

/// Aggregate per-cleaner reclaimed bytes from macleaner's table into the panel's
/// three buckets: Caches, Trash, Logs & temp.
pub fn parse_breakdown(output: &str) -> Vec<(String, u64)> {
    let (mut caches, mut trash, mut logs) = (0u64, 0u64, 0u64);
    for line in output.lines() {
        let l = line.trim();
        if !l.starts_with('\u{2713}') {
            continue; // only "✓ <cleaner> <size>" rows
        }
        let bytes = parse_size_field(l).unwrap_or(0);
        let lower = l.to_lowercase();
        if lower.contains("trash") {
            trash += bytes;
        } else if lower.contains("log") || lower.contains("tmp") || lower.contains("simctl") {
            logs += bytes;
        } else {
            caches += bytes;
        }
    }
    vec![
        ("Caches".to_string(), caches),
        ("Trash".to_string(), trash),
        ("Logs & temp".to_string(), logs),
    ]
}

// ── JS payloads (camelCase to match panel.html) ───────────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelState {
    pub onboarding: bool,
    pub free_gb: u64,
    pub total_gb: u64,
    pub used_pct: u64,
    pub warn: bool,
    pub last_clean: String,
    pub start_at_login: bool,
    pub accent: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub pct: u64,
    pub reclaimed: String,
    pub label: String,
}

#[derive(Serialize)]
pub struct Bucket {
    pub name: String,
    pub amount: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResultsPayload {
    pub reclaimed_gb: String,
    pub free_now_gb: u64,
    pub free_before_gb: u64,
    pub buckets: Vec<Bucket>,
}

/// `window.mbState({...});`
pub fn js_state(s: &PanelState) -> String {
    format!("window.mbState({});", serde_json::to_string(s).unwrap_or_default())
}
/// `window.mbProgress({...});`
pub fn js_progress(p: &Progress) -> String {
    format!("window.mbProgress({});", serde_json::to_string(p).unwrap_or_default())
}
/// `window.mbResults({...});`
pub fn js_results(r: &ResultsPayload) -> String {
    format!("window.mbResults({});", serde_json::to_string(r).unwrap_or_default())
}
/// `window.mbToast("…");` (string safely JSON-escaped).
pub fn js_toast(text: &str) -> String {
    format!("window.mbToast({});", serde_json::to_string(text).unwrap_or_else(|_| "\"\"".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn used_pct_ac2() {
        assert_eq!(used_pct(33, 256), 87);
        assert_eq!(used_pct(0, 256), 100);
        assert_eq!(used_pct(256, 256), 0);
        assert_eq!(used_pct(33, 0), 0); // no divide-by-zero
        assert!(used_pct(999, 256) <= 100); // clamped
    }

    #[test]
    fn action_parse_ac3() {
        assert_eq!(Action::parse("cleanNow"), Some(Action::CleanNow));
        assert_eq!(Action::parse("preview"), Some(Action::Preview));
        assert_eq!(Action::parse("openLog"), Some(Action::OpenLog));
        assert_eq!(Action::parse("toggleLogin"), Some(Action::ToggleLogin));
        assert_eq!(Action::parse("done"), Some(Action::Done));
        assert_eq!(Action::parse("quit"), Some(Action::Quit));
        assert_eq!(Action::parse("ready"), Some(Action::Ready));
        assert_eq!(Action::parse("bogus"), None);
    }

    #[test]
    fn reclaimed_parse_ac4() {
        assert!(parse_reclaimed_bytes("  ── reclaimed 4.2 GB across 8 items") > 0);
        assert!(parse_reclaimed_bytes("would reclaim 1.5 KB across 2 items") >= 1500);
        assert_eq!(parse_reclaimed_bytes("nothing here"), 0);
        // 4.2 GB ≈ 4.5e9 bytes
        let b = parse_reclaimed_bytes("reclaimed 4.2 GB");
        assert!(b > 4_000_000_000 && b < 5_000_000_000);
        // concatenated form ("4.2GB") also parses
        assert!(parse_reclaimed_bytes("reclaimed 4.2GB across 8 items") > 4_000_000_000);
    }

    #[test]
    fn onboarding_primary_is_a_dry_run() {
        // The prominent first-run button must perform a no-delete preview, never
        // a real clean (red-team: it was wired to cleanNow).
        assert!(PANEL_HTML.contains("onclick=\"act('preview')\">Run a safe preview"));
        assert!(!PANEL_HTML.contains("onclick=\"act('cleanNow')\">Run a safe preview"));
    }

    #[test]
    fn breakdown_buckets() {
        let out = "\
  ✓ uv cache prune       2.0 GB  (1 item)
  ✓ library caches       100 MB  (3 items)
  ✓ old logs             0 B  (6 items)
  ✓ trash                1.4 GB  (2 items)";
        let b = parse_breakdown(out);
        assert_eq!(b.len(), 3);
        assert_eq!(b[0].0, "Caches");
        assert!(b[0].1 > 2_000_000_000); // uv + library caches
        assert_eq!(b[1].0, "Trash");
        assert!(b[1].1 > 1_000_000_000);
    }

    #[test]
    fn panel_html_has_anchors_ac5() {
        for id in ["screen-idle", "screen-cleaning", "screen-results", "screen-onboarding"] {
            assert!(PANEL_HTML.contains(&format!("id=\"{id}\"")), "missing {id}");
        }
        assert!(PANEL_HTML.contains("webkit.messageHandlers"));
    }

    #[test]
    fn js_payloads_are_valid() {
        let s = PanelState {
            onboarding: false,
            free_gb: 33,
            total_gb: 256,
            used_pct: 87,
            warn: false,
            last_clean: "2h ago".into(),
            start_at_login: true,
            accent: "#0a84ff".into(),
        };
        let js = js_state(&s);
        assert!(js.starts_with("window.mbState({"));
        assert!(js.contains("\"freeGb\":33"));
        assert!(js.contains("\"startAtLogin\":true"));
        // toast escaping
        assert_eq!(js_toast("a\"b"), "window.mbToast(\"a\\\"b\");");
    }
}
