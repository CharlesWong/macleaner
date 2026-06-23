# macleaner

A small macOS disk-and-memory hygiene suite for Apple Silicon Macs, written in
Rust. Two binaries in one Cargo workspace:

| Crate | Binary | What it is |
|---|---|---|
| [`daemon/`](daemon/) | `macleaner` | A safe, idempotent **daily disk cleaner** that runs as a launchd LaunchAgent. Reclaims regenerating junk (package-manager caches, stale temp/log files, download caches, the Trash) on the boot disk — and never deletes anything that isn't trivially re-created. |
| [`bar/`](bar/) | `macleaner-bar` | A **menu-bar companion**: a native `WKWebView` panel showing boot-disk free space, a one-click clean flow, and a safe memory-relief view. Auto-starts at login via SMAppService. |

The two are **loosely coupled** — `macleaner-bar` shells out to `~/bin/macleaner`
and reads its state; it imports nothing from the daemon. They live in one repo so
the daemon's output format and the bar's parser stay in sync.

## Build

```bash
cargo build --release            # builds both crates
cargo test                       # all unit + integration tests
cargo clippy --all-targets -- -D warnings
```

## Install

```bash
# the daily cleaner (LaunchAgent)
./target/release/macleaner install

# the menu-bar app (login item)
./target/release/macleaner-bar install
```

See [`daemon/README.md`](daemon/README.md) and [`bar/README.md`](bar/README.md)
for details, safety model, and the launchd / SMAppService specifics.

## Safety

Both tools are built so the destructive operations are conservative and
auditable:

- The cleaner only touches an allowlist of regenerating caches, age-gates every
  deletion, never follows symlinks off-target, and never deletes the model
  caches relocated to `/Volumes/External`. Its first run is a no-delete preview.
- The menu-bar app's memory view never runs `purge` or other snake-oil; "Quit
  selected" sends a graceful `SIGTERM` only to apps you tick, and the kill path
  re-validates every pid so it can never target `pid <= 1`, itself, or a
  critical system process.

## License

MIT.
