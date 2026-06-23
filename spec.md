# macleaner-bar — spec (Native panel)

**Goal:** A macOS menu-bar companion for `macleaner` whose click opens a native WKWebView panel (disk ring, Clean-now flow, results, onboarding) and that auto-starts at login via SMAppService.

## Behavior

A menu-bar (NSStatusItem, via `tray-icon`) with **no Dock icon** (`LSUIElement`).
The title shows boot-disk free space (`🧹 NNG`, `⚠️` below `min_free_gb`).
**Clicking the status item toggles a native panel** — a borderless, transparent
`NSPanel` positioned under the menu-bar item, hosting a **WKWebView** that renders
the imported design (`Macleaner Bar - Native`). The WebView is bridged to Rust via
a `WKScriptMessageHandler`; live data is injected and actions run the real
macleaner. No browser chrome — it is a genuine AppKit/WebKit panel.

Panel screens (from the design):

- **Idle:** a disk ring (free GB of 256, ~87% used arc; orange when below
  `min_free_gb`), `NN GB free of 256 GB`, a status row (green/orange dot,
  "Healthy/Low space · last clean <ago>"), a primary **Clean now** button,
  **Preview** + **Open log** buttons, and a **Start at login** toggle.
- **Cleaning:** spinning ring with percent, "Pruning <label>…", "<n> GB
  reclaimed", a progress bar — shown while `macleaner run --force` runs.
- **Results:** green ring "reclaimed +N GB", "Free space now NN GB · up from MM
  GB", a per-bucket breakdown (caches / trash / logs+temp), a **Done** button.
- **Onboarding (first run):** hero + three assurances (never touches your files /
  runs once a day / preview first) + **Install & start at login**.
- **Toast:** Preview shows "Dry-run · would reclaim ~N GB".

It is **loosely coupled** to macleaner: it shells out to `~/bin/macleaner`
(`run --force`, `dry-run`), reads macleaner's last-run + `min_free_gb`, computes
free space via `statvfs`, and imports nothing from macleaner. The clean runs on a
background thread; progress/results return to the WebView on the main thread.

## Auto-start (SMAppService)

`install` builds `~/Applications/Macleaner Bar.app` (Info.plist `LSUIElement`,
bundle id `show.laowang.macleaner-bar`, `CFBundleExecutable=macleaner-bar`) and
launches it; the bundled app registers a Login Item via `SMAppService`
(bidirectional reconcile from `~/.config/macleaner-bar/config.toml`).

## Acceptance criteria (testable)

- **AC1:** `title(35,25)`=="🧹 35G"; `title(18,25)`=="⚠️ 18G".
- **AC2:** `used_pct(33, 256)` rounds to 87 (used = (total-free)/total); `used_pct`
  is clamped to 0..=100 and never divides by zero when total is 0.
- **AC3:** `Action::parse("cleanNow")`==Some(CleanNow); also preview/openLog/
  toggleLogin/done/quit/ready; unknown → None.
- **AC4:** `parse_reclaimed_bytes("…reclaimed 4.2 GB across…")` extracts a positive
  byte total; missing → 0.
- **AC5:** the embedded panel HTML (`PANEL_HTML`) contains the screen anchors
  `id="screen-idle"`, `id="screen-cleaning"`, `id="screen-results"`,
  `id="screen-onboarding"` and the bridge hook `webkit.messageHandlers`.
- **AC6:** `free_gb($HOME)` via `statvfs` is positive.
- **AC7:** login-item status maps: `SMAppServiceStatus::Enabled`→`is_enabled()`
  true; reconcile is bidirectional.
- **AC8:** `ago_label`: 90s→"just now", 300→"5m ago", 7200→"2h ago", 259200→"3d ago".

## Architecture

- `disk.rs`, `state.rs`, `ui.rs`, `barconfig.rs`, `loginitem.rs`, `bundle.rs`,
  `actions.rs` — as before (pure/loose-coupling helpers).
- `bridge.rs` — pure: `Action` enum + `parse`, `used_pct`, `parse_reclaimed_bytes`,
  the JSON state payloads; `PANEL_HTML` via `include_str!("panel.html")`.
- `panel.rs` — objc2: build the transparent `NSPanel` + `WKWebView` +
  `WKScriptMessageHandler`, show/hide under a screen rect, inject state, push
  progress/results/toast by evaluating JS.
- `main.rs` — tao loop + tray-icon status item; on click toggle the panel; a
  channel carries background clean progress/results back to the main thread.

## Testing

Unit tests cover every pure helper (AC1–AC8 minus the objc2 panel). The panel is
verified by launching the app, clicking the item, and `screencapture`-ing the
panel. Native UI, so Puppeteer/simulators do not apply.
