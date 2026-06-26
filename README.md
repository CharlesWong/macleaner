# macleaner

[![CI](https://github.com/CharlesWong/macleaner/actions/workflows/ci.yml/badge.svg)](https://github.com/CharlesWong/macleaner/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/CharlesWong/macleaner?sort=semver)](https://github.com/CharlesWong/macleaner/releases)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

A small, **safety-first** macOS disk-and-memory hygiene suite, written in Rust.
Two binaries in one Cargo workspace:

| Crate | Binary | What it is |
|---|---|---|
| [`daemon/`](daemon/) | `macleaner` | A safe, idempotent **daily disk cleaner** (launchd LaunchAgent). Reclaims only regenerating junk — package-manager caches, stale temp/logs, download caches, the Trash — never anything that isn't trivially re-created. |
| [`bar/`](bar/) | `macleaner-bar` | A native **menu-bar companion**: a `WKWebView` panel with boot-disk free space, a one-click clean flow, and a safe memory-relief view. Auto-starts at login. |

They share one repo so the daemon's output and the bar's parser stay in sync;
`macleaner-bar` just shells out to `~/bin/macleaner` and imports nothing from it.

> **Why another cleaner?** It cleans only regenerating junk on a tight
> allowlist, previews before deleting, is fully open-source, and its memory view
> never runs `purge` — the opposite of cleaners that over-reach and snake-oil
> "RAM boosters".

## Install

### From a release (signed + notarized — just run it)

Download the universal (Apple Silicon + Intel) binaries from the
[latest release](https://github.com/CharlesWong/macleaner/releases/latest):

```sh
tar -xzf macleaner-*-universal-macos.tar.gz
./macleaner install        # daily disk-cleaner LaunchAgent (runs at 03:00 + at login)
./macleaner-bar install    # menu-bar app + Login Item
```

The release binaries are **signed with a Developer ID and notarized by Apple**,
so macOS Gatekeeper runs them without any quarantine workaround. (Building from
source yourself produces an unsigned binary — see [`docs/SIGNING.md`](docs/SIGNING.md).)

### From source

```sh
git clone https://github.com/CharlesWong/macleaner
cd macleaner
cargo build --release         # builds both binaries
./target/release/macleaner install
./target/release/macleaner-bar install
```

After install the daemon cleans daily at 03:00; the menu-bar 🧹 shows free space
— click it for the panel (Clean / Preview / memory). If a cleaner can't reach a
protected folder, grant **Full Disk Access** to `/usr/libexec/xpcproxy` +
`/sbin/launchd` (System Settings → Privacy). Uninstall: `macleaner uninstall` /
`macleaner-bar uninstall`.

## Safety model — the whole point

Every destructive operation is conservative and auditable:

- **The cleaner** only touches an **allowlist of roots** (caches, stale tmp,
  old logs, the Trash), **age-gates** every deletion, **never follows symlinks**
  off-target, and **never deletes anything under `/Volumes`**. Its **first run
  is a no-delete preview**, and a once-per-day guard makes "run at login" safe.
- **The memory view never runs `purge`** or other snake-oil. "Quit selected"
  sends a graceful `SIGTERM` only to apps you tick, and the kill path
  **re-validates every pid** so it can never target `pid <= 1`, itself, or a
  critical system process (WindowServer / launchd / Dock / …).

Per-component detail lives in [`daemon/`](daemon/) and [`bar/`](bar/).

## Develop

```sh
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

CI runs these on macOS for every push/PR; see
[`CONTRIBUTING.md`](CONTRIBUTING.md) for the non-negotiable safety rules.

## License

[MIT](LICENSE) © Charles Wong.
