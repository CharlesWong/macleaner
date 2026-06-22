# Deploying macleaner-bar

`macleaner-bar install` does everything below; this explains what it does and why.

## Install

```bash
cargo build --release
./target/release/macleaner-bar install
```

1. Builds `~/Applications/Macleaner Bar.app`:
   - `Contents/MacOS/macleaner-bar` — the binary, copied off `/Volumes/External`
     onto the boot drive.
   - `Contents/Info.plist` — `LSUIElement=true` (menu-bar agent, no Dock icon),
     `CFBundleIdentifier=show.laowang.macleaner-bar`,
     `CFBundleExecutable=macleaner-bar`.
2. `open`s the bundle. On launch the app reconciles its Login Item from
   `~/.config/macleaner-bar/config.toml` (`start_at_login`, default `true`) via
   `SMAppService.mainAppService().register()`.

## Why a real `.app` bundle

SMAppService Login Items are keyed on a bundle's identity (`CFBundleIdentifier`),
so the running process must live inside a proper `.app` bundle — a bare binary
cannot register a main-app Login Item. `LSUIElement` keeps it out of the Dock and
app switcher. Building under `~/Applications` keeps the binary on the boot drive
(launchd/Login Items don't run binaries from external volumes reliably).

## Auto-start behavior

- Registered as a Login Item ⇒ macOS launches the app at every login/reboot.
- The **Start at login** menu item flips `start_at_login` and calls
  `register()` / `unregister()`.
- Shows under **System Settings → General → Login Items** (where the user can
  also disable it).

## Verifying

```bash
ls -d ~/Applications/"Macleaner Bar.app"          # bundle exists
pgrep -fl "Macleaner Bar"                          # running
# Menu bar shows "🧹 NNg"; System Settings → General → Login Items lists it.
```

## Uninstall

```bash
./target/release/macleaner-bar uninstall   # stops it, unregisters, removes the bundle
```
