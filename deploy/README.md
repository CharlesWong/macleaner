# Deploying macleaner as a launchd LaunchAgent

`macleaner install` automates all of this; this document explains what it does
and the macOS rules it obeys, plus the one manual step it cannot do for you.

## What `install` does

```bash
cargo build --release
./target/release/macleaner install
```

1. Copies the release binary to **`~/bin/macleaner`** (boot drive).
2. Writes **`~/bin/macleaner-launcher.sh`** (a `/bin/sh` launcher that sets a
   rich PATH and `exec`s the binary with `run`).
3. Writes **`~/Library/LaunchAgents/show.laowang.macleaner.plist`**.
4. Creates **`~/Library/Logs/macleaner/`** and points the agent's stdout/stderr
   there.
5. `launchctl bootout` (any old instance) then `launchctl bootstrap gui/$UID`.

The agent runs **at load** (catch-up after reboot) and **daily at 03:00**
(configurable via `schedule_hour`/`schedule_minute`). The binary's once-per-day
guard means a load-time run and the scheduled run never double-clean.

## Why the binary lives on `~/bin`, not `/Volumes/External`

macOS launchd/xpcproxy refuse to spawn ad-hoc-signed Mach-O binaries that live
on an external volume (`posix_spawn` EPERM, surfaced as `last exit code = 78`).
The source tree stays on `/Volumes/External`; only the built binary is copied to
the boot drive. The agent is also invoked as `/bin/sh <launcher>` rather than
the binary directly, so the launcher's `com.apple.provenance` xattr doesn't trip
Gatekeeper review. No `WorkingDirectory` points at `/Volumes`. These are the
external-volume LaunchAgent rules; macleaner follows all of them.

## The one manual step — Full Disk Access (only if needed)

macleaner's targets (`~/.cache`, `/opt/homebrew`, `~/Library/Caches`,
`~/Library/Logs`, `~/.Trash`) are user-owned and normally need **no** special
permission. If a future cleaner reaches a TCC-protected folder and the log shows
permission errors, grant Full Disk Access:

> System Settings → Privacy & Security → Full Disk Access → `+` →
> `Cmd-Shift-G` → `/usr/libexec/xpcproxy` → Open → toggle ON. Repeat for
> `/sbin/launchd`.

## Verifying

```bash
launchctl print gui/$UID/show.laowang.macleaner | grep -E "state|last exit|runs"
./target/release/macleaner status            # last-run timestamp advances
tail -n 20 ~/Library/Logs/macleaner/macleaner.log
```

A healthy agent shows `state = running` briefly each run, a recent `last exit
code = 0`, and fresh lines in the log. If `runs` increments but the log never
grows, suspect the external-volume rules above (most often Full Disk Access).

## Uninstall

```bash
./target/release/macleaner uninstall   # unloads + removes plist & launcher
```

The `~/bin/macleaner` binary is left in place for manual use.
