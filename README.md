# macleaner-bar

A macOS **menu-bar companion** for [`macleaner`](../macleaner). It shows boot-disk
free space in the menu bar and gives you one-click cleans — and auto-starts at
login as a modern SMAppService Login Item.

## What it shows

The menu-bar title is your boot-disk free space, e.g. **🧹 33G** — turning
**⚠️** red when free space drops below macleaner's `min_free_gb` threshold.
Clicking opens:

```
Free: 33 GB
Last clean: 2h ago
─────────────
Clean now        → macleaner run --force, then a notification with the result
Dry-run          → macleaner dry-run, notification with "would reclaim …"
Open log         → opens ~/Library/Logs/macleaner/macleaner.log
─────────────
Start at login ✓ (toggle)
Quit
```

It refreshes every 60 s. It is **loosely coupled** to macleaner: it shells out to
`~/bin/macleaner`, reads macleaner's last-run timestamp and `min_free_gb`, and
computes free space itself via `statvfs`. It imports nothing from macleaner and
changes nothing about it.

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
cargo test                          # 14 unit tests (every acceptance criterion)
cargo clippy --all-targets -- -D warnings
cargo build --release
```

Stack: `tray-icon` + `tao` (menu bar + event loop), `objc2-service-management`
(SMAppService), `statvfs` for free space. Built under the `company/` checklist
harness (spec → build → ship) with real cargo gates and an adversarial commit
critic. See [`deploy/README.md`](deploy/README.md) for install details.
