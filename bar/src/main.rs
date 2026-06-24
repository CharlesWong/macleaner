//! macleaner-bar — a macOS menu-bar companion for macleaner with a native
//! WKWebView panel.

mod actions;
mod barconfig;
mod bridge;
mod bundle;
mod disk;
mod loginitem;
mod mem;
mod panel;
mod state;
mod ui;

use barconfig::BarConfig;
use bridge::{Action, Progress};
use objc2::MainThreadMarker;
use panel::Panel;
use serde::Deserialize;
use std::path::Path;
use std::process::Command;
use std::sync::mpsc::{channel, Sender};
use std::time::{Duration, Instant};
use tao::event::Event;
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tray_icon::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

fn main() -> anyhow::Result<()> {
    let home = state::home()?;
    match std::env::args().nth(1).as_deref() {
        Some("install") => cmd_install(&home),
        Some("uninstall") => cmd_uninstall(&home),
        Some("-h") | Some("--help") => {
            println!("macleaner-bar [install|uninstall]  (no args: run the menu-bar app)");
            Ok(())
        }
        _ => run_tray(home),
    }
}

#[derive(Deserialize)]
struct Msg {
    action: String,
    #[serde(default)]
    pids: Vec<i32>,
}

/// Messages from the background clean/preview threads back to the main loop.
enum CleanMsg {
    Done(String),
    DryDone(String),
}

/// Cleaning animation state. phase: 0 idle, 1 waiting on the clean thread,
/// 2 animating the real result up to 100%.
struct Clean {
    phase: u8,
    anim_start: Instant,
    reclaimed_gb: f64,
    free_before: u64,
    results: Option<bridge::ResultsPayload>,
}

fn run_tray(home: std::path::PathBuf) -> anyhow::Result<()> {
    let cfg = BarConfig::load(&home);
    reconcile_login(&cfg);
    let mtm = MainThreadMarker::new().expect("menu-bar app must run on the main thread");

    let event_loop = EventLoopBuilder::<()>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let free = disk::free_gb(&home).unwrap_or(0);
    let min = state::read_min_free_gb(&home);
    let tray = TrayIconBuilder::new()
        .with_title(ui::title(free, min))
        .with_tooltip("macleaner")
        .build()?;

    let mut panel = Panel::new(mtm, proxy);
    // Debug affordance: MACLEANER_BAR_OPEN=1 opens the panel at launch (for
    // screenshots / manual testing) without needing a status-item click.
    if std::env::var_os("MACLEANER_BAR_OPEN").is_some() {
        panel.show(1900.0);
    }
    let tray_rx = TrayIconEvent::receiver();
    let (res_tx, res_rx) = channel::<CleanMsg>();
    let mut clean = Clean {
        phase: 0,
        anim_start: Instant::now(),
        reclaimed_gb: 0.0,
        free_before: 0,
        results: None,
    };
    let mut next_refresh = Instant::now() + Duration::from_secs(120);
    // When the panel was last hidden. Used to debounce the status-item click:
    // clicking the item resigns the key panel (→ dismiss) AND fires a tray click;
    // without this the tray click would immediately reopen it.
    let mut last_hide: Option<Instant> = None;

    event_loop.run(move |event, _target, control_flow| {
        // The panel resigning key (clicking outside / Cmd-Tab) or Escape is
        // delivered as a user event → dismiss, like a normal status-bar popover.
        if let Event::UserEvent(()) = event {
            if panel.visible {
                panel.hide();
                last_hide = Some(Instant::now());
            }
        }

        *control_flow = ControlFlow::WaitUntil(next_refresh);

        // periodic menu-bar title refresh
        if Instant::now() >= next_refresh {
            let f = disk::free_gb(&home).unwrap_or(0);
            tray.set_title(Some(ui::title(f, state::read_min_free_gb(&home))));
            next_refresh = Instant::now() + Duration::from_secs(120);
        }

        // status-item clicks toggle the panel
        while let Ok(ev) = tray_rx.try_recv() {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                rect,
                ..
            } = ev
            {
                let cx = rect.position.x + rect.size.width as f64 / 2.0;
                let just_dismissed =
                    last_hide.is_some_and(|t| t.elapsed() < Duration::from_millis(350));
                if !panel.visible && just_dismissed {
                    // this same click already dismissed the panel (resign-key);
                    // consume it so the panel doesn't immediately reopen.
                    last_hide = None;
                } else if panel.visible {
                    panel.hide();
                    last_hide = Some(Instant::now());
                } else {
                    // open it (a freshly built panel requests its state via 'ready')
                    panel.show(cx);
                }
            }
        }

        // actions posted by the WebView
        for raw in panel.poll_actions() {
            handle_action(&raw, &panel, &home, &res_tx, &mut clean, control_flow);
        }

        // results from background threads
        while let Ok(msg) = res_rx.try_recv() {
            match msg {
                CleanMsg::Done(out) => {
                    let free_now = disk::free_gb(&home).unwrap_or(clean.free_before);
                    let bytes = bridge::parse_reclaimed_bytes(&out);
                    clean.reclaimed_gb = bytes as f64 / 1024f64.powi(3);
                    let buckets = bridge::parse_breakdown(&out)
                        .into_iter()
                        .map(|(name, b)| bridge::Bucket {
                            name,
                            amount: bridge::format_bytes(b),
                        })
                        .collect();
                    clean.results = Some(bridge::ResultsPayload {
                        reclaimed_gb: format!("{:.1}", clean.reclaimed_gb),
                        free_now_gb: free_now,
                        free_before_gb: clean.free_before,
                        buckets,
                    });
                    clean.phase = 2;
                    clean.anim_start = Instant::now();
                }
                CleanMsg::DryDone(s) => panel.eval(&bridge::js_toast(s.trim())),
            }
        }

        // drive the cleaning animation
        tick_clean(&panel, &mut clean, control_flow);

        // Footprint: once the panel is closed and nothing is cleaning, tear down
        // the WebView so its WebKit XPC processes terminate (idle ≈ 38 MB, no
        // WebKit). Rebuilt on the next open.
        if panel.is_built() && !panel.visible && clean.phase == 0 {
            panel.release();
        }
    });
}

fn tick_clean(panel: &Panel, clean: &mut Clean, control_flow: &mut ControlFlow) {
    match clean.phase {
        1 => {
            // waiting on the real clean: gentle ramp to 25%
            let e = clean.anim_start.elapsed().as_millis() as u64;
            let pct = (e / 30).min(25);
            panel.eval(&bridge::js_progress(&Progress {
                pct,
                reclaimed: "0.0".into(),
                label: "package caches".into(),
            }));
            *control_flow = ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(80));
        }
        2 => {
            // animate the REAL reclaimed total up to 100% over ~1s
            let e = clean.anim_start.elapsed().as_millis() as f64;
            let pct = ((e / 1000.0) * 100.0).min(100.0);
            if pct < 100.0 {
                let rec = clean.reclaimed_gb * pct / 100.0;
                let label = match pct as u64 {
                    0..=29 => "package caches",
                    30..=54 => "the Homebrew cache",
                    55..=79 => "the Cargo registry",
                    _ => "the Trash",
                };
                panel.eval(&bridge::js_progress(&Progress {
                    pct: pct as u64,
                    reclaimed: format!("{:.1}", rec),
                    label: label.into(),
                }));
                *control_flow = ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(60));
            } else {
                if let Some(r) = clean.results.take() {
                    panel.eval(&bridge::js_results(&r));
                }
                clean.phase = 0;
            }
        }
        _ => {}
    }
}

fn handle_action(
    raw: &str,
    panel: &Panel,
    home: &Path,
    res_tx: &Sender<CleanMsg>,
    clean: &mut Clean,
    control_flow: &mut ControlFlow,
) {
    let Ok(msg) = serde_json::from_str::<Msg>(raw) else {
        return;
    };
    let Some(action) = Action::parse(&msg.action) else {
        return;
    };
    match action {
        Action::Ready | Action::Done => send_state(panel, home),
        Action::Mem => send_mem(panel),
        Action::QuitPids => {
            mem::quit_pids(&msg.pids);
            send_mem(panel);
        }
        Action::OpenLog => actions::open_log(home),
        Action::ToggleLogin => {
            toggle_login_intent(home);
            send_state(panel, home);
        }
        Action::Preview => {
            panel.eval(&bridge::js_toast("Running dry-run…"));
            let tx = res_tx.clone();
            let h = home.to_path_buf();
            std::thread::spawn(move || {
                let _ = tx.send(CleanMsg::DryDone(actions::run_dry(&h)));
            });
        }
        Action::CleanNow => {
            if clean.phase != 0 {
                return;
            }
            clean.free_before = disk::free_gb(home).unwrap_or(0);
            clean.phase = 1;
            clean.anim_start = Instant::now();
            let tx = res_tx.clone();
            let h = home.to_path_buf();
            std::thread::spawn(move || {
                let _ = tx.send(CleanMsg::Done(actions::run_capture(
                    &h,
                    &["run", "--force"],
                )));
            });
        }
        Action::Quit => *control_flow = ControlFlow::Exit,
    }
}

fn send_state(panel: &Panel, home: &Path) {
    let free = disk::free_gb(home).unwrap_or(0);
    let min = state::read_min_free_gb(home);
    let last = state::read_last_run(home);
    let now = state::now_epoch();
    let total = disk::total_gb(home).unwrap_or(bridge::TOTAL_GB);
    let st = bridge::PanelState {
        onboarding: last.is_none(),
        free_gb: free,
        total_gb: total,
        used_pct: bridge::used_pct(free, total),
        warn: free < min,
        last_clean: match last {
            Some(t) => state::ago_label(now.saturating_sub(t)),
            None => "never".into(),
        },
        start_at_login: loginitem::is_enabled(),
        accent: "#0a84ff".into(),
    };
    panel.eval(&bridge::js_state(&st));
}

fn send_mem(panel: &Panel) {
    let level = mem::pressure_level();
    let apps = mem::top_consumers(8)
        .into_iter()
        .map(|a| bridge::AppRow {
            name: a.name,
            mb: a.mb,
            pid: a.pid,
            killable: a.killable,
        })
        .collect();
    let m = bridge::MemState {
        level,
        level_label: mem::pressure_label(level).to_string(),
        swap_gb: format!("{:.1}", mem::swap_used_gb()),
        apps,
    };
    panel.eval(&bridge::js_mem(&m));
}

fn toggle_login_intent(home: &Path) {
    let mut cfg = BarConfig::load(home);
    cfg.start_at_login = !cfg.start_at_login;
    let _ = cfg.save(home);
    let _ = if cfg.start_at_login {
        loginitem::register()
    } else {
        loginitem::unregister()
    };
}

fn reconcile_login(cfg: &BarConfig) {
    match loginitem::reconcile_action(cfg.start_at_login, loginitem::is_enabled()) {
        Some(true) => {
            let _ = loginitem::register();
        }
        Some(false) => {
            let _ = loginitem::unregister();
        }
        None => {}
    }
}

fn cmd_install(home: &Path) -> anyhow::Result<()> {
    let exe = std::env::current_exe()?;
    let app = bundle::install(home, &exe)?;
    println!("installed bundle → {}", app.display());
    Command::new("open").arg(&app).status()?;
    println!(
        "launched. Click the menu-bar item to open the panel. It also appears \
         under System Settings → General → Login Items."
    );
    Ok(())
}

/// Escape extended-regex metacharacters so a literal string is matched verbatim
/// by `pkill -f`. macOS `pgrep`/`pkill` match patterns as **extended** regular
/// expressions by default (see `man pgrep` / re_format(3) — verified: `sl(e)ep`
/// matches the `sleep` process), so ERE escaping is the correct dialect here.
/// Without this, the `.` in "Macleaner Bar.app" would match any character.
fn ere_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if r".^$*+?()[]{}|\".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn cmd_uninstall(home: &Path) -> anyhow::Result<()> {
    // `pkill -f` matches its argument as an extended regex; escape it so the
    // dot stays literal. Matching the bundle name targets the installed
    // menu-bar app (its path contains "Macleaner Bar.app") and not the CLI
    // process running this uninstall (whose path does not).
    let _ = Command::new("pkill")
        .args(["-f", &ere_escape(bundle::APP_DIR_NAME)])
        .status();
    let _ = loginitem::unregister();
    bundle::uninstall(home)?;
    println!("uninstalled (Login Item removed, app bundle deleted).");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::ere_escape;

    #[test]
    fn ere_escape_makes_dot_literal() {
        assert_eq!(ere_escape("Macleaner Bar.app"), r"Macleaner Bar\.app");
    }

    #[test]
    fn ere_escape_of_app_dir_name_is_safe_literal() {
        // The exact value handed to `pkill -f` in cmd_uninstall: a metacharacter
        // sneaking into APP_DIR_NAME would otherwise become an unintended regex.
        assert_eq!(
            ere_escape(crate::bundle::APP_DIR_NAME),
            r"Macleaner Bar\.app"
        );
    }

    #[test]
    fn ere_escape_handles_all_metacharacters() {
        assert_eq!(ere_escape("a.b*c+d?"), r"a\.b\*c\+d\?");
        assert_eq!(ere_escape("(x)[y]{z}"), r"\(x\)\[y\]\{z\}");
        // anchors, alternation, and backslash itself
        assert_eq!(ere_escape(r"a^b$c|d\e"), r"a\^b\$c\|d\\e");
        assert_eq!(ere_escape("plain"), "plain");
    }
}
