# macleaner-bar

A macOS **menu-bar companion** for [`macleaner`](../macleaner). It shows boot-disk
free space in the menu bar and gives you one-click cleans — and auto-starts at
login as a modern SMAppService Login Item.

## What it shows

The menu-bar title is your boot-disk free space, e.g. **🧹 33G** — turning
**⚠️** red when free space drops below macleaner's `min_free_gb` threshold.

Clicking the item opens a **native panel** (a borderless, transparent `NSPanel`
hosting a `WKWebView` that renders the *Macleaner Bar – Native* design, bridged
to Rust). It has four screens:

- **Idle** — a disk-usage ring ("33 GB free" of 256 GB, orange below threshold),
  a health/last-clean line, and **Clean now** · **Preview** · **Open log**
  buttons plus a **Start at login** toggle.
- **Cleaning** — a spinning ring with live percent, the cleaner being pruned, the
  reclaimed total, and a progress bar (shown while `macleaner run --force` runs).
- **Results** — "reclaimed +N GB", "Free space now NN GB · up from MM GB", and a
  caches / trash / logs breakdown.
- **First-run onboarding** — what it does and a safe-preview entry point.

It is **loosely coupled** to macleaner: it shells out to `~/bin/macleaner`
(`run --force`, `dry-run`), reads macleaner's last-run timestamp and
`min_free_gb`, and computes free space itself via `statvfs`. It imports nothing
from macleaner and changes nothing about it. The clean runs on a background
thread; progress and results return to the WebView on the main thread. Light and
dark mode follow the system; menu-bar title refreshes every 60 s.

> Tip: `MACLEANER_BAR_OPEN=1 macleaner-bar` opens the panel at launch (handy for
> screenshots / testing) without a status-item click.

### Footprint

The WebView is **built lazily** — only when you first open the panel — and **torn
down** when the panel closes while idle, so its WebKit processes terminate. A
menu-bar app launched at login and left untouched therefore holds **zero** WebKit
processes and sits at ~35 MB (the Rust process alone, ~0 % CPU); WebKit (~25 MB)
exists only while the panel is actually on screen. The idle title refresh is a
single 120 s timer.

## Install (auto-start at login)

```bash
cargo build --release
./target/release/macleaner-bar install
```

`install` builds `~/Applications/Macleaner Bar.app` (a real `.app` bundle with
`LSUIElement` so there's no Dock icon), copies the binary in, and launches it.
A real bundle is required because SMAppService Login Items key on the app's
bundle identity. On first launch it registers itself as a Login Item (default
on), so it starts on every login/reboot; the **Start at login** menu item toggles
that, and it appears under **System Settings → General → Login Items**.

```bash
./target/release/macleaner-bar            # run in the foreground (dev)
./target/release/macleaner-bar uninstall  # remove Login Item + app bundle
```

## Development

```bash
cargo test                          # 19 unit tests (every acceptance criterion)
cargo clippy --all-targets -- -D warnings
cargo build --release
```

Stack: `tray-icon` + `tao` (status item + event loop), `objc2-app-kit` +
`objc2-web-kit` (the transparent `NSPanel` + `WKWebView` + a
`WKScriptMessageHandler` bridge), `objc2-service-management` (SMAppService),
`statvfs` for free space. The panel UI is `src/panel.html`, rendered in the
WebView and driven by the Rust bridge in `src/bridge.rs`. Built under the
`company/` checklist harness (spec → build → ship) with real cargo gates and an
adversarial commit critic. See [`deploy/README.md`](deploy/README.md) for install
details.
