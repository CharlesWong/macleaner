# macleaner-bar — spec

**Goal:** A macOS menu-bar companion for `macleaner` that shows boot-disk free space, triggers cleans, and auto-starts at login via SMAppService.

## Behavior

A menu-bar (NSStatusItem) app — **no Dock icon** (`LSUIElement`). The title shows
boot-disk free space; clicking opens a dropdown:

```
Free: 35 GB                 (disabled status row)
Last clean: 2h ago
─────────────
Clean now    → ~/bin/macleaner run --force, then notify the summary
Dry-run      → ~/bin/macleaner dry-run, then notify "would reclaim X"
Open log     → open ~/Library/Logs/macleaner/macleaner.log
─────────────
Start at login ✓ (toggle)
Quit
```

It is **loosely coupled** to macleaner: it shells out to `~/bin/macleaner`, reads
macleaner's last-run timestamp (`~/.local/state/macleaner/last-run`) and config
(`min_free_gb`), and computes free space itself via `statvfs`. It never imports
macleaner and makes no change to it. The title shows a warning glyph when free
space is below `min_free_gb`. A 60 s timer refreshes the title.

## Auto-start (SMAppService Login Item)

Registering a Login Item requires a real `.app` bundle. The `install` subcommand
builds `~/Applications/Macleaner Bar.app` (Info.plist with `LSUIElement=true`,
bundle id `show.laowang.macleaner-bar`, `CFBundleExecutable=macleaner-bar`),
copies the binary in, and `open`s it. The bundled app reconciles the Login Item
on launch from its own config (`~/.config/macleaner-bar/config.toml`,
`start_at_login`, default true) via `SMAppService.mainAppService()`
register/unregister; the "Start at login" menu item toggles it. `uninstall`
unregisters and removes the bundle.

## Acceptance criteria (testable)

- **AC1:** `title(35, 25)` == `"🧹 35G"`; `title(18, 25)` == `"⚠️ 18G"` (the warning
  glyph appears only when `free_gb < min_free_gb`).
- **AC2:** `ago_label` renders `90` s → `"just now"`, `5*60` → `"5m ago"`,
  `2*3600` → `"2h ago"`, `3*86400` → `"3d ago"`.
- **AC3:** `last_clean_label(None, now)` == `"Last clean: never"`; with a
  timestamp ~2 h old it contains `"2h ago"`.
- **AC4:** `info_plist(...)` contains `LSUIElement`/`<true/>`, the bundle id
  `show.laowang.macleaner-bar`, and `CFBundleExecutable`.
- **AC5:** `read_min_free_gb` returns 25 when the macleaner config is absent, and
  the configured value when present (`min_free_gb = 10` → 10).
- **AC6:** `free_gb($HOME)` via `statvfs` returns a positive number.
- **AC7:** the Login Item status maps correctly: `SMAppServiceStatus::Enabled`
  → `is_enabled()` true; `NotRegistered` → false.

## Scope (out)

- No changes to macleaner; no shared crate (loose coupling only).
- No preferences window, no CPU/RAM stats, no history graphs (just free space +
  last-clean + actions).
- The cleaning logic itself lives entirely in macleaner.

## Architecture

- `disk.rs` — `free_bytes`/`free_gb` via `statvfs` (small, copied pattern).
- `state.rs` — `read_last_run`, `ago_label`, `read_min_free_gb` (parse macleaner's
  TOML), `home`.
- `ui.rs` — pure label builders: `title`, `free_label`, `last_clean_label`.
- `actions.rs` — spawn macleaner (`run --force`/`dry-run`), `open_log`, `notify`
  (osascript).
- `loginitem.rs` — `register`/`unregister`/`is_enabled` via SMAppService.
- `bundle.rs` — `info_plist`, `install` (.app bundle), `uninstall`.
- `barconfig.rs` — read/write `start_at_login`.
- `main.rs` — arg dispatch (`install`/`uninstall`/default run); tao event loop +
  tray-icon + muda menu + 60 s timer.

## Testing

Unit tests cover every pure function (AC1–AC7 minus the GUI loop). The tray UI is
verified by launching the app and capturing the menu bar (`screencapture`) to
confirm the item renders, plus checking the Login Item registered. Native macOS
UI, so Puppeteer/simulators do not apply.
