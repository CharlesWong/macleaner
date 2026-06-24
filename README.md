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

### From a release (no toolchain needed)

Download the universal (Apple Silicon + Intel) binaries from the
[latest release](https://github.com/CharlesWong/macleaner/releases/latest):

```sh
tar -xzf macleaner-*-universal-macos.tar.gz
# the app isn't Apple-notarized yet, so clear the download quarantine BEFORE
# installing — the copy macleaner-bar places in ~/Applications inherits it:
xattr -dr com.apple.quarantine macleaner macleaner-bar
./macleaner install        # daily disk-cleaner LaunchAgent (runs at 03:00 + at login)
./macleaner-bar install    # menu-bar app + Login Item
# already ran install before clearing? also strip the installed bundle:
xattr -dr com.apple.quarantine ~/Applications/"Macleaner Bar.app"
```

> **Gatekeeper:** the binaries are currently **unsigned / un-notarized**. The
> `xattr` line above clears the quarantine flag so macOS doesn't block the app.
> See [`docs/SIGNING.md`](docs/SIGNING.md) for the notarization plan.

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
