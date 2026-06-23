# macleaner

A safe, idempotent daily disk cleaner for macOS, written in Rust and run as a
launchd LaunchAgent. It reclaims regenerating junk (package-manager caches,
stale temp/log files, download caches, the Trash) on the boot disk — and is
built to **never** delete anything that isn't trivially re-created.

## Why

A 256 GB Mac mini boot disk fills repeatedly. Large model caches were relocated
to `/Volumes/External/caches/` (huggingface, whisper, puppeteer); those are kept
data. macleaner clears the regenerating remainder, daily, unattended.

## Safety model

This daemon deletes files on a schedule, so safety is the design center:

- **Allowlist roots.** Each cleaner acts only inside its own hard-coded root; a
  target is rejected unless it canonicalizes to a path *inside* that root.
- **Kept-data deny.** Anything resolving under `/Volumes/` is refused — the
  relocated caches and their `~/.cache/{huggingface,whisper,puppeteer}` symlinks
  can never be deleted or traversed into.
- **No symlink traversal.** Directory walks never follow symlinks.
- **Age gating.** A file is removed only if older than the cleaner's threshold.
- **Dry-run honesty.** `dry-run` mutates nothing yet reports the real totals.
  The first run after install is a no-delete preview.
- **Once-per-day guard.** A timestamp gates real runs, so `RunAtLoad` safely
  catches up after a reboot without over-running.
- **Fail-soft.** A failing cleaner logs and never aborts the others; absent
  tools are skipped. Every run is logged to `~/Library/Logs/macleaner/`.

## Cleaners

**Daily tier (always):** `uv cache prune` (skipped while `uv` is running),
`npm cache verify`, `pnpm store prune`, `xcrun simctl delete unavailable`,
`brew cleanup`, stale temp files >7 d (e.g. `~/.gemini/tmp`), logs >30 d under
`~/Library/Logs`.

**Sweep tier (only when boot-disk free < `min_free_gb`, default 25):** prune
`~/Library/Caches` >14 d (Apple caches excluded), `pip cache purge` + cargo
registry cache/src >14 d, empty `~/.Trash` >30 d.

## Usage

```bash
cargo build --release
./target/release/macleaner status        # config, last run, free space
./target/release/macleaner dry-run       # show what would be freed
./target/release/macleaner run           # clean (first run is a preview)
./target/release/macleaner run --force   # clean now, ignore the guard
./target/release/macleaner install       # install the daily LaunchAgent
./target/release/macleaner uninstall     # remove the LaunchAgent
```

Configuration lives at `~/.config/macleaner/config.toml` (written by
`init-config` / `install`). Every cleaner is individually toggleable and
age-tunable; `min_free_gb`, the schedule, and the guard interval are all there.

See [`deploy/README.md`](deploy/README.md) for the launchd install details and
the macOS external-volume rules this project follows.

## Development

```bash
cargo test                          # 22 tests incl. every acceptance criterion
cargo clippy --all-targets -- -D warnings
cargo build --release
```

This project was built under the `company/` checklist harness (spec → build →
ship), gated by real `cargo` quality checks and an adversarial commit critic.
