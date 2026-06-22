# macleaner — spec

**Goal:** A safe, idempotent Rust daemon that reclaims macOS boot-disk space every day via a launchd LaunchAgent, never deleting kept data.

## Safety model (load-bearing — this daemon deletes files on a schedule)

1. **Allowlist roots, not denylist.** Each cleaner operates only on paths under
   its own hard-coded root (e.g. `~/.cache/uv`, `~/Library/Logs`). A computed
   deletion target is rejected unless it canonicalizes to a path *inside* that
   root (`is_within_allowed_root`).
2. **Kept-data deny rule.** Any path resolving under `/Volumes/` is refused by
   the safety layer regardless of cleaner — the relocated huggingface / whisper
   / puppeteer caches on the external drive can never be deleted, and the
   symlinks at `~/.cache/{huggingface,whisper,puppeteer}` can never be
   traversed into.
3. **No symlink traversal off-target.** Directory walks never follow symlinks;
   a symlink is never deleted-through.
4. **Age gating.** A file is deleted only if its modification time is older than
   the cleaner's threshold (days). Files modified within the window are never
   touched.
5. **Dry-run honesty.** `--dry-run` performs zero filesystem mutations yet
   reports the identical byte total a real run would free. The first run after
   install defaults to dry-run.
6. **Idempotent / once-per-day guard.** A timestamp file records the last
   successful run; `run` exits early if it ran within ~20 h (unless `--force`),
   so `RunAtLoad` is safe (catches up after reboot, never over-runs).
7. **Fail-soft & isolated.** A cleaner that errors logs and does not abort the
   others; external-tool cleaners are skipped cleanly when the tool is absent.
   Every run appends a structured per-cleaner summary to `~/Library/Logs/macleaner/`.

## Acceptance criteria (testable)

- **AC1:** `format_bytes(1536)` returns `"1.5 KB"`.
- **AC2:** Given a dir with one file mtime 10 days old and one mtime 1 day old,
  the age-gated scan (threshold 7 d) selects exactly the 10-day file and reports
  its size; the 1-day file is untouched.
- **AC3:** In dry-run, that scenario reports the identical byte total **and**
  leaves both files on disk (zero deletions).
- **AC4:** `is_within_allowed_root` rejects a path under `/Volumes/...` and any
  path that escapes the cleaner root (kept-data deny), even passed explicitly.
- **AC5:** A directory walk does not descend through a symlink pointing outside
  the cleaner root.
- **AC6:** Once-per-day guard: last-run 1 h old ⇒ `should_run` false; 25 h old or
  `--force` ⇒ true.
- **AC7:** The generated launchd plist uses `/bin/sh` + a launcher script (not
  the binary directly), points log paths under `~/Library/Logs`, sets no
  `WorkingDirectory` on `/Volumes/`, and includes a daily `StartCalendarInterval`.

## Scope (in)

CLI `macleaner` with subcommands: `run`, `dry-run`, `status`, `install`,
`uninstall`, `init-config`. Cleaners run in two tiers:

- **Daily tier (always, cheap, idempotent):** `uv cache prune` (skipped if a
  `uv` process is running), `npm cache verify`, `pnpm store prune`,
  `xcrun simctl delete unavailable`, `brew cleanup`, stale temp files >7 d (e.g.
  `~/.gemini/tmp`), log files >30 d under `~/Library/Logs`.
- **Sweep tier (only when boot-disk free < `min_free_gb`, default 25):** prune
  `~/Library/Caches` entries >14 d (Apple-owned / locked entries excluded),
  `pip cache purge` + cargo registry cache/src >14 d, and empty `~/.Trash`
  items >30 d.

## Scope (out)

- Anything requiring `sudo`/root (root-owned simulator caches) — manual only.
- Relocating data (already done) and the `/Volumes/External` caches (denied).
- GUI, networking, telemetry.

## Architecture

- `config.rs` — TOML config at `~/.config/macleaner/config.toml`; `min_free_gb`,
  per-cleaner enable + age thresholds.
- `safety.rs` — `is_within_allowed_root`, `/Volumes` kept-data deny, age
  predicate, `format_bytes`.
- `disk.rs` — boot-disk free bytes via `statvfs($HOME)`.
- `report.rs` — per-cleaner + total byte accounting and structured log line.
- `cleaners/` — one module per cleaner implementing a `Cleaner` trait
  (`name`, `tier`, `run(ctx) -> CleanReport`), each bounded to its root.
- `runner.rs` — selects tier by free-space threshold, enforces the once-per-day
  guard, runs cleaners fail-soft, writes the log + timestamp.
- `launchd.rs` — plist + launcher generation, `install` / `uninstall`.
- `main.rs` — clap CLI.
