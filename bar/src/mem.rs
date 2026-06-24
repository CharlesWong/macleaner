//! Safe, user-directed memory relief: read macOS memory pressure + swap + the
//! top memory-consuming apps, and gracefully quit user-selected apps. No
//! `purge`, no snake-oil, no auto-kill, never targets pid<=1 / self / critical
//! system processes.

use std::process::Command;

/// One process as reported by `ps`.
#[derive(Debug, Clone)]
pub struct Proc {
    pub pid: i32,
    pub rss_kb: u64,
    pub path: String,
}

/// One aggregated app (its processes summed).
#[derive(Debug, Clone)]
pub struct AppMem {
    pub name: String,
    pub mb: u64,
    pub pid: i32,
    pub killable: bool,
}

/// Critical processes we will never offer to kill (plus our own app).
const CRITICAL: &[&str] = &[
    "WindowServer",
    "loginwindow",
    "launchd",
    "kernel_task",
    "SystemUIServer",
    "Dock",
    "Finder",
    "ControlCenter",
    "NotificationCenter",
    "Spotlight",
    "coreaudiod",
    "macleaner-bar",
    "Macleaner Bar",
];

/// Map the macOS pressure level (1/2/4) to a label.
pub fn pressure_label(level: u8) -> &'static str {
    match level {
        1 => "Normal",
        2 => "Warning",
        4 => "Critical",
        _ => "Unknown",
    }
}

/// Parse the GB used from `vm.swapusage` output ("… used = 11303.38M …").
pub fn parse_swap_used_gb(s: &str) -> f64 {
    if let Some(idx) = s.find("used =") {
        for tok in s[idx + 6..].split_whitespace() {
            let num: String = tok
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            if let Ok(v) = num.parse::<f64>() {
                let mb = if tok.contains('G') {
                    v * 1024.0
                } else if tok.contains('K') {
                    v / 1024.0
                } else {
                    v // M (the macOS default)
                };
                return mb / 1024.0;
            }
        }
    }
    0.0
}

/// The owning app name for a process path: the first `*.app` bundle component,
/// else the executable's basename.
pub fn app_name_of(path: &str) -> String {
    for comp in path.split('/') {
        if let Some(name) = comp.strip_suffix(".app") {
            return name.to_string();
        }
    }
    path.rsplit('/').next().unwrap_or(path).trim().to_string()
}

/// True if `path` is an app's MAIN process (≤1 `.app` component), not a helper
/// nested inside a `*Helper.app`.
pub fn is_main_process(path: &str) -> bool {
    path.split('/').filter(|c| c.ends_with(".app")).count() <= 1
}

/// May we offer to SIGTERM this? Never pid<=1, never ourselves, never a critical
/// system process.
pub fn is_killable(name: &str, pid: i32, self_pid: i32) -> bool {
    pid > 1 && pid != self_pid && !CRITICAL.iter().any(|c| name.eq_ignore_ascii_case(c))
}

/// Group processes by owning app, summing RSS; the representative pid is the
/// app's largest MAIN process (so quitting it takes its helpers with it).
pub fn aggregate(procs: Vec<Proc>, self_pid: i32) -> Vec<AppMem> {
    use std::collections::HashMap;
    struct Acc {
        rss: u64,
        main_pid: Option<i32>,
        main_rss: u64,
        any_pid: i32,
        any_rss: u64,
    }
    let mut map: HashMap<String, Acc> = HashMap::new();
    for p in procs {
        let name = app_name_of(&p.path);
        let main = is_main_process(&p.path);
        let e = map.entry(name).or_insert(Acc {
            rss: 0,
            main_pid: None,
            main_rss: 0,
            any_pid: p.pid,
            any_rss: 0,
        });
        e.rss += p.rss_kb;
        if main && p.rss_kb >= e.main_rss {
            e.main_pid = Some(p.pid);
            e.main_rss = p.rss_kb;
        }
        if p.rss_kb >= e.any_rss {
            e.any_pid = p.pid;
            e.any_rss = p.rss_kb;
        }
    }
    let mut out: Vec<AppMem> = map
        .into_iter()
        .map(|(name, a)| {
            let pid = a.main_pid.unwrap_or(a.any_pid);
            let killable = is_killable(&name, pid, self_pid);
            AppMem {
                mb: a.rss / 1024,
                pid,
                killable,
                name,
            }
        })
        .collect();
    out.sort_by_key(|x| std::cmp::Reverse(x.mb));
    out
}

// ── impure: live system reads ────────────────────────────────────────────────

fn sysctl_n(key: &str) -> Option<String> {
    let o = Command::new("sysctl").args(["-n", key]).output().ok()?;
    o.status
        .success()
        .then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

pub fn pressure_level() -> u8 {
    sysctl_n("kern.memorystatus_vm_pressure_level")
        .and_then(|s| s.parse().ok())
        .unwrap_or(1)
}

pub fn swap_used_gb() -> f64 {
    sysctl_n("vm.swapusage")
        .map(|s| parse_swap_used_gb(&s))
        .unwrap_or(0.0)
}

fn parse_ps_line(line: &str) -> Option<Proc> {
    let line = line.trim_start();
    let (pid_s, rest) = line.split_once(char::is_whitespace)?;
    let rest = rest.trim_start();
    let (rss_s, comm) = rest.split_once(char::is_whitespace)?;
    Some(Proc {
        pid: pid_s.parse().ok()?,
        rss_kb: rss_s.parse().ok()?,
        path: comm.trim().to_string(),
    })
}

/// Top `limit` memory-consuming apps (aggregated), largest first.
pub fn top_consumers(limit: usize) -> Vec<AppMem> {
    let self_pid = std::process::id() as i32;
    let out = match Command::new("ps")
        .args(["-axo", "pid=,rss=,comm="])
        .output()
    {
        Ok(o) => String::from_utf8_lossy(&o.stdout).into_owned(),
        Err(_) => return Vec::new(),
    };
    let procs: Vec<Proc> = out.lines().filter_map(parse_ps_line).collect();
    let mut apps = aggregate(procs, self_pid);
    apps.truncate(limit);
    apps
}

/// Resolve a pid to its owning-app name (via `ps`), or None if it's gone.
fn process_name(pid: i32) -> Option<String> {
    let o = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "comm="])
        .output()
        .ok()?;
    if !o.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&o.stdout).trim().to_string();
    (!path.is_empty()).then(|| app_name_of(&path))
}

/// Gracefully SIGTERM the given pids. The pid list is NOT trusted: each pid is
/// independently re-resolved to its process name and re-checked with
/// `is_killable`, so a critical/self/pid<=1 target is refused even if asked.
/// Returns how many signals were sent.
pub fn quit_pids(pids: &[i32]) -> usize {
    let self_pid = std::process::id() as i32;
    let mut n = 0;
    for &pid in pids {
        let Some(name) = process_name(pid) else {
            continue;
        };
        if is_killable(&name, pid, self_pid) {
            // SAFETY: kill(2) with SIGTERM; pid re-validated above.
            if unsafe { libc::kill(pid, libc::SIGTERM) } == 0 {
                n += 1;
            }
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pressure_label_ac13() {
        assert_eq!(pressure_label(1), "Normal");
        assert_eq!(pressure_label(2), "Warning");
        assert_eq!(pressure_label(4), "Critical");
    }

    #[test]
    fn swap_parse_ac14() {
        let g =
            parse_swap_used_gb("total = 12288.00M  used = 11303.38M  free = 984.62M  (encrypted)");
        assert!(g > 10.5 && g < 11.5, "got {g}");
        assert_eq!(parse_swap_used_gb("nope"), 0.0);
    }

    #[test]
    fn app_name_ac9() {
        assert_eq!(
            app_name_of("/Applications/Google Chrome.app/Contents/Frameworks/Google Chrome Helper.app/Contents/MacOS/Google Chrome Helper"),
            "Google Chrome"
        );
        assert_eq!(app_name_of("/usr/local/bin/node"), "node");
        assert_eq!(app_name_of("/Users/cw/.local/bin/claude"), "claude");
    }

    #[test]
    fn is_main_ac10() {
        assert!(is_main_process(
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
        ));
        assert!(!is_main_process(
            "/Applications/Google Chrome.app/Contents/Frameworks/X Helper.app/Contents/MacOS/X"
        ));
        assert!(is_main_process("/usr/local/bin/node"));
    }

    #[test]
    fn aggregate_sums_helpers_ac11() {
        let procs = vec![
            Proc {
                pid: 455,
                rss_kb: 262144,
                path: "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome".into(),
            },
            Proc {
                pid: 500,
                rss_kb: 524288,
                path: "/Applications/Google Chrome.app/Contents/Frameworks/H.app/Contents/MacOS/H"
                    .into(),
            },
            Proc {
                pid: 9,
                rss_kb: 1024,
                path: "/usr/local/bin/node".into(),
            },
        ];
        let apps = aggregate(procs, 1234);
        let chrome = apps.iter().find(|a| a.name == "Google Chrome").unwrap();
        assert_eq!(chrome.pid, 455, "representative is the main process");
        assert_eq!(chrome.mb, (262144 + 524288) / 1024, "helpers summed");
        assert!(chrome.killable);
        // largest first
        assert_eq!(apps[0].name, "Google Chrome");
    }

    #[test]
    fn live_reads_smoke() {
        // exercises the real ps/sysctl path on this machine
        let apps = top_consumers(8);
        assert!(!apps.is_empty(), "expected some processes");
        assert!(apps[0].mb > 0, "top app should use memory");
        assert!(apps.iter().any(|a| a.killable), "expected a killable app");
        assert!(
            matches!(pressure_level(), 1 | 2 | 4),
            "unexpected pressure level"
        );
        assert!(swap_used_gb() >= 0.0);
    }

    #[test]
    fn is_killable_ac12() {
        assert!(!is_killable("WindowServer", 100, 50));
        assert!(!is_killable("launchd", 1, 50));
        assert!(!is_killable("Google Chrome", 50, 50)); // self
        assert!(!is_killable("Google Chrome", 1, 50)); // pid<=1
        assert!(!is_killable("Macleaner Bar", 100, 50)); // our own app
        assert!(is_killable("Google Chrome", 455, 50));
    }
}
