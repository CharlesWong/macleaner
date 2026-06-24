# macleaner — workspace spec (context for review)

A Cargo workspace with two macOS binaries:

- **`daemon/` → `macleaner`**: a safe daily disk-cleaner LaunchAgent. Prunes only
  regenerating junk (package-manager caches, stale tmp, old logs,
  `~/Library/Caches`, pip/cargo download caches, Trash) on an allowlist of
  roots, age-gated, never following symlinks off-target, never touching
  `/Volumes`. First run is a no-delete preview; a once-per-day guard makes
  `RunAtLoad` safe.
- **`bar/` → `macleaner-bar`**: a native menu-bar app (NSStatusItem + a
  transparent, key-capable NSPanel hosting a WKWebView). Shows boot-disk free
  space, a clean flow with live progress, and a safe memory-relief view
  (pressure + swap + top apps with a user-confirmed graceful SIGTERM that
  re-validates every pid and never targets pid<=1/self/critical processes, and
  never runs `purge`). Auto-starts via SMAppService.

The two are loosely coupled (the bar shells out to `~/bin/macleaner`).

This launch-readiness pass adds CI, universal-binary releases, signing/
notarization docs, a license, contributing guide, and a polished README — see
`LAUNCH.md` for the criteria. No behavior changes to the shipped tools beyond
formatting; the safety model is unchanged.
