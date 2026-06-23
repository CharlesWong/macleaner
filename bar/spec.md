# macleaner-bar — spec (Native panel + Memory)

**Goal:** A macOS menu-bar companion for `macleaner` whose click opens a native WKWebView panel (disk ring, clean flow, results, onboarding, and a safe memory-pressure view), auto-starting at login via SMAppService.

## Behavior

A menu-bar (NSStatusItem via `tray-icon`), **no Dock icon** (`LSUIElement`). The
title shows boot-disk free space (`🧹 NNG`, `⚠️` below `min_free_gb`). Clicking
toggles a **native panel** — a borderless transparent `NSPanel` hosting a
`WKWebView` rendering the design, bridged to Rust via `WKScriptMessageHandler`.
The WebView is **built lazily** on first open and **released** when the panel is
closed while idle (zero WebKit processes when unopened).

Disk screens: **idle** (ring, Clean now / Preview / Open log, Start-at-login
toggle), **cleaning** (live progress), **results** (reclaimed + breakdown),
**onboarding** (first run; the prominent button is a no-delete preview).

**Memory view (safe, user-directed):** opened from the idle screen, it shows
macOS **memory pressure** (Normal / Warning / Critical, from
`kern.memorystatus_vm_pressure_level` = 1 / 2 / 4), **swap used**, and the **top
memory-consuming apps** — aggregated from `ps` so helper processes are summed
under their parent app. The user ticks apps and taps **Quit selected**, which
sends a graceful **SIGTERM** to each app's **main** process only. It NEVER
auto-kills, NEVER runs `purge` or other snake-oil, NEVER targets `pid <= 1` or
its own process, and excludes critical system processes (WindowServer,
loginwindow, launchd, kernel_task). macOS reclaims the freed memory.

It is **loosely coupled** to macleaner (shells out to `~/bin/macleaner`, reads
last-run + `min_free_gb`, computes free space via `statvfs`); the memory view
uses only `sysctl`/`ps`/`kill`. It imports nothing from macleaner.

## Auto-start (SMAppService)

`install` builds `~/Applications/Macleaner Bar.app` (Info.plist `LSUIElement`,
bundle id `show.laowang.macleaner-bar`) and registers a Login Item via
`SMAppService` (bidirectional reconcile from `config.toml`).

## Acceptance criteria (testable)

- **AC1:** `title(35,25)`=="🧹 35G"; `title(18,25)`=="⚠️ 18G".
- **AC2:** `used_pct(33,256)`==87; `used_pct(33,0)`==0 (no div-by-zero); clamped 0..=100.
- **AC3:** `Action::parse` maps cleanNow/preview/openLog/toggleLogin/done/quit/
  ready/mem/quitPids; unknown → None.
- **AC4:** `parse_reclaimed_bytes` handles "4.2 GB" and "4.2GB"; missing → 0.
- **AC5:** `PANEL_HTML` contains `id="screen-idle/cleaning/results/onboarding/
  memory"` and `webkit.messageHandlers`.
- **AC6:** `free_gb`/`total_gb` via `statvfs` positive; total ≥ free.
- **AC7:** login-item status maps; reconcile is bidirectional.
- **AC8:** `ago_label`: 90→"just now", 300→"5m ago", 7200→"2h ago", 259200→"3d ago".
- **AC9 (mem):** `app_name_of("/Applications/Google Chrome.app/Contents/Frameworks/
  X Helper.app/Contents/MacOS/Y")`=="Google Chrome"; of "/usr/local/bin/node"=="node".
- **AC10 (mem):** `is_main_process` true when the path has one `.app`, false when
  it's inside a nested `*Helper.app` (two `.app`).
- **AC11 (mem):** aggregating Chrome main + helpers sums their RSS under one
  "Google Chrome" entry whose pid is the main process's pid.
- **AC12 (mem):** `is_killable` is false for "WindowServer"/"launchd"/`pid<=1`/the
  own pid, true for "Google Chrome".
- **AC13 (mem):** `pressure_label(1/2/4)` == Normal/Warning/Critical.
- **AC14 (mem):** `parse_swap_used_gb("total = 12288.00M  used = 11303.38M ...")`
  ≈ 11.0.

## Architecture

- `disk.rs`, `state.rs`, `ui.rs`, `barconfig.rs`, `loginitem.rs`, `bundle.rs`,
  `actions.rs`, `bridge.rs`, `panel.rs`, `main.rs` — as before.
- `mem.rs` — pure: `pressure_label`, `parse_swap_used_gb`, `app_name_of`,
  `is_main_process`, `aggregate`, `is_killable`; impure: `top_consumers` (ps),
  `pressure_level`/`swap_used_gb` (sysctl), `quit_pids` (guarded SIGTERM).
- `bridge.rs` adds the `Mem`/`QuitPids` actions + the memory JSON payloads.
- `panel.rs` builds the WebView lazily; `main.rs` handles `mem`/`quitPids`.

## Testing

Unit tests cover every pure helper (AC1–AC14 minus the objc2 panel and the live
ps/sysctl/kill). The panel is verified by launching + `screencapture`. Native
UI, so Puppeteer/simulators do not apply.
