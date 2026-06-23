//! macleaner-bar — a macOS menu-bar companion for macleaner with a native
//! WKWebView panel.

mod actions;
mod barconfig;
mod bridge;
mod bundle;
mod disk;
mod loginitem;
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

    let event_loop = EventLoopBuilder::new().build();

    let free = disk::free_gb(&home).unwrap_or(0);
    let min = state::read_min_free_gb(&home);
    let tray = TrayIconBuilder::new()
        .with_title(ui::title(free, min))
        .with_tooltip("macleaner")
        .build()?;

    let mut panel = Panel::new(mtm);
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

    event_loop.run(move |_event, _target, control_flow| {
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
                panel.toggle(cx);
                // A freshly built panel requests its state via the 'ready'
                // message once its HTML loads, so no eager send_state is needed.
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
                        .map(|(name, b)| bridge::Bucket { name, amount: bridge::format_bytes(b) })
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
                let _ = tx.send(CleanMsg::Done(actions::run_capture(&h, &["run", "--force"])));
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

fn cmd_uninstall(home: &Path) -> anyhow::Result<()> {
    let _ = Command::new("pkill").args(["-f", bundle::APP_DIR_NAME]).status();
    let _ = loginitem::unregister();
    bundle::uninstall(home)?;
    println!("uninstalled (Login Item removed, app bundle deleted).");
    Ok(())
}
