//! macleaner-bar — a macOS menu-bar companion for macleaner.

mod actions;
mod barconfig;
mod bundle;
mod disk;
mod loginitem;
mod state;
mod ui;

use barconfig::BarConfig;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::TrayIconBuilder;

fn main() -> anyhow::Result<()> {
    let home = state::home()?;
    match std::env::args().nth(1).as_deref() {
        Some("install") => cmd_install(&home),
        Some("uninstall") => cmd_uninstall(&home),
        Some("-h") | Some("--help") => {
            println!("macleaner-bar [install|uninstall]  (no args: run the menu-bar app)");
            Ok(())
        }
        _ => run_tray(home), // diverges into the event loop
    }
}

fn cmd_install(home: &Path) -> anyhow::Result<()> {
    let exe = std::env::current_exe()?;
    let app = bundle::install(home, &exe)?;
    println!("installed bundle → {}", app.display());
    Command::new("open").arg(&app).status()?;
    println!(
        "launched. It shows in the menu bar and (after first launch) under \
         System Settings → General → Login Items."
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

/// Reconcile the Login Item with the saved intent, both directions — so an
/// out-of-band edit to `start_at_login` is honored on the next launch. A
/// no-op/harmless error when not running from the `.app` bundle.
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

fn run_tray(home: std::path::PathBuf) -> anyhow::Result<()> {
    let cfg = BarConfig::load(&home);
    reconcile_login(&cfg);

    let min = state::read_min_free_gb(&home);
    let free = disk::free_gb(&home).unwrap_or(0);
    let now = state::now_epoch();
    let last = state::read_last_run(&home);

    let event_loop = EventLoopBuilder::new().build();

    let free_item = MenuItem::new(ui::free_label(free), false, None);
    let last_item = MenuItem::new(ui::last_clean_label(last, now), false, None);
    let clean_item = MenuItem::new("Clean now", true, None);
    let dry_item = MenuItem::new("Dry-run", true, None);
    let log_item = MenuItem::new("Open log", true, None);
    let login_item = CheckMenuItem::new("Start at login", true, cfg.start_at_login, None);
    let quit_item = MenuItem::new("Quit", true, None);

    let menu = Menu::new();
    menu.append_items(&[
        &free_item,
        &last_item,
        &PredefinedMenuItem::separator(),
        &clean_item,
        &dry_item,
        &log_item,
        &PredefinedMenuItem::separator(),
        &login_item,
        &PredefinedMenuItem::separator(),
        &quit_item,
    ])?;

    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_title(ui::title(free, min))
        .with_tooltip("macleaner")
        .build()?;

    let (clean_id, dry_id, log_id, login_id, quit_id) = (
        clean_item.id().clone(),
        dry_item.id().clone(),
        log_item.id().clone(),
        login_item.id().clone(),
        quit_item.id().clone(),
    );
    let menu_rx = MenuEvent::receiver();
    let mut next_refresh = Instant::now() + Duration::from_secs(60);

    event_loop.run(move |_event, _target, control_flow| {
        *control_flow = ControlFlow::WaitUntil(next_refresh);

        if Instant::now() >= next_refresh {
            refresh(&home, &tray, &free_item, &last_item);
            next_refresh = Instant::now() + Duration::from_secs(60);
        }

        while let Ok(ev) = menu_rx.try_recv() {
            if ev.id == clean_id {
                let summary = actions::run_clean(&home);
                actions::notify("macleaner — cleaned", &summary);
                refresh(&home, &tray, &free_item, &last_item);
            } else if ev.id == dry_id {
                let summary = actions::run_dry(&home);
                actions::notify("macleaner — dry run", &summary);
            } else if ev.id == log_id {
                actions::open_log(&home);
            } else if ev.id == login_id {
                toggle_login(&home, &login_item);
            } else if ev.id == quit_id {
                *control_flow = ControlFlow::Exit;
            }
        }
    });
}

fn refresh(home: &Path, tray: &tray_icon::TrayIcon, free_item: &MenuItem, last_item: &MenuItem) {
    let min = state::read_min_free_gb(home);
    let free = disk::free_gb(home).unwrap_or(0);
    let now = state::now_epoch();
    let last = state::read_last_run(home);
    tray.set_title(Some(ui::title(free, min)));
    free_item.set_text(ui::free_label(free));
    last_item.set_text(ui::last_clean_label(last, now));
}

fn toggle_login(home: &Path, login_item: &CheckMenuItem) {
    let mut cfg = BarConfig::load(home);
    cfg.start_at_login = !cfg.start_at_login;
    let _ = cfg.save(home);
    let res = if cfg.start_at_login {
        loginitem::register()
    } else {
        loginitem::unregister()
    };
    match res {
        Ok(()) => login_item.set_checked(cfg.start_at_login),
        Err(e) => {
            actions::notify("macleaner — login item", &format!("{e}"));
            login_item.set_checked(loginitem::is_enabled());
        }
    }
}
