//! macleaner — a safe, idempotent daily macOS disk cleaner.

mod cleaners;
mod config;
mod disk;
mod launchd;
mod report;
mod runner;
mod safety;

use clap::{Parser, Subcommand};
use config::{home_dir, Config};
use runner::RunOpts;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Parser)]
#[command(name = "macleaner", version, about = "Safe, idempotent daily macOS disk cleaner")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run enabled cleaners (respects config + the once-per-day guard).
    Run {
        /// Ignore the once-per-day guard and the first-run dry preview.
        #[arg(long)]
        force: bool,
    },
    /// Show what would be freed; delete nothing.
    DryRun,
    /// Show config, last run, free space, and install state.
    Status,
    /// Install the launchd LaunchAgent (copies the binary to ~/bin).
    Install,
    /// Remove the launchd LaunchAgent.
    Uninstall,
    /// Write a default config file if none exists.
    InitConfig,
}

fn main() -> anyhow::Result<()> {
    augment_path();
    let home = home_dir()?;
    match Cli::parse().cmd {
        Cmd::Run { force } => cmd_run(&home, force),
        Cmd::DryRun => cmd_dry_run(&home),
        Cmd::Status => cmd_status(&home),
        Cmd::Install => cmd_install(&home),
        Cmd::Uninstall => cmd_uninstall(&home),
        Cmd::InitConfig => cmd_init_config(&home),
    }
}

fn cmd_run(home: &std::path::Path, force: bool) -> anyhow::Result<()> {
    let cfg = Config::load(home)?;

    // Safety: the very first invocation on a fresh install is a no-delete
    // preview, then marks itself initialized so scheduled runs clean normally.
    if !force && runner::read_last_run(home).is_none() {
        if let Some(rep) = runner::execute(&cfg, home, &RunOpts { dry_run: true, force: false })? {
            print!("{}", rep.render());
        }
        runner::write_last_run(home, SystemTime::now())?;
        println!(
            "\nFirst run was a PREVIEW (no deletions). Scheduled runs will clean; \
             run `macleaner run --force` to clean right now."
        );
        return Ok(());
    }

    match runner::execute(&cfg, home, &RunOpts { dry_run: false, force })? {
        Some(rep) => print!("{}", rep.render()),
        None => println!("macleaner: nothing to do (disabled, or already ran within the guard window)."),
    }
    Ok(())
}

fn cmd_dry_run(home: &std::path::Path) -> anyhow::Result<()> {
    let cfg = Config::load(home)?;
    match runner::execute(&cfg, home, &RunOpts { dry_run: true, force: true })? {
        Some(rep) => print!("{}", rep.render()),
        None => println!("macleaner: disabled in config."),
    }
    Ok(())
}

fn cmd_status(home: &std::path::Path) -> anyhow::Result<()> {
    let cfg = Config::load(home)?;
    let free = disk::free_gb(home).unwrap_or(0);
    println!("macleaner status");
    println!("  enabled:        {}", cfg.enabled);
    println!("  free space:     {free} GB (sweep below {} GB)", cfg.min_free_gb);
    println!("  schedule:       daily {:02}:{:02}", cfg.schedule_hour, cfg.schedule_minute);
    println!("  guard interval: {} h", cfg.min_interval_hours);
    match runner::read_last_run(home) {
        Some(t) => {
            let secs = t.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
            let ago = SystemTime::now().duration_since(t).map(|d| d.as_secs() / 3600).unwrap_or(0);
            println!("  last run:       {secs} (epoch s, ~{ago} h ago)");
        }
        None => println!("  last run:       never"),
    }
    let installed = launchd::plist_path(home).exists();
    println!("  launchd agent:  {}", if installed { "installed" } else { "not installed" });
    println!("  config file:    {}", config::config_path(home).display());
    println!("  log file:       {}", runner::log_path(home).display());
    Ok(())
}

fn cmd_install(home: &std::path::Path) -> anyhow::Result<()> {
    let cfg = Config::load(home)?;
    let (cfg_path, created) = Config::ensure_file(home)?;
    println!("config: {} ({})", cfg_path.display(), if created { "created" } else { "exists" });

    let source = std::env::current_exe()?;
    let steps = launchd::install(home, &source, cfg.schedule_hour, cfg.schedule_minute)?;
    for s in &steps {
        println!("  {s}");
    }

    println!("\nPreview of the next run (no deletions):");
    if let Some(rep) = runner::execute(&cfg, home, &RunOpts { dry_run: true, force: true })? {
        print!("{}", rep.render());
    }
    println!(
        "\nInstalled. The agent runs at load and daily {:02}:{:02}. \
         If a cleaner can't reach a protected folder, grant Full Disk Access to \
         /usr/libexec/xpcproxy and /sbin/launchd in System Settings → Privacy & Security.",
        cfg.schedule_hour, cfg.schedule_minute
    );
    Ok(())
}

fn cmd_uninstall(home: &std::path::Path) -> anyhow::Result<()> {
    for s in launchd::uninstall(home)? {
        println!("  {s}");
    }
    println!("Uninstalled. (~/bin/macleaner left in place for manual use.)");
    Ok(())
}

fn cmd_init_config(home: &std::path::Path) -> anyhow::Result<()> {
    let (path, created) = Config::ensure_file(home)?;
    println!("config {}: {}", if created { "created" } else { "already exists" }, path.display());
    Ok(())
}

/// launchd hands processes a minimal PATH; prepend the dirs where our cleaner
/// tools actually live so `tool_exists`/exec find them.
fn augment_path() {
    let home = std::env::var("HOME").unwrap_or_default();
    let mut parts = vec![
        "/opt/homebrew/bin".to_string(),
        format!("{home}/.local/bin"),
        "/usr/local/bin".to_string(),
        format!("{home}/bin"),
    ];
    if let Ok(cur) = std::env::var("PATH") {
        parts.push(cur);
    }
    std::env::set_var("PATH", parts.join(":"));
}
