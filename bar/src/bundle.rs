//! Generate and install the `.app` bundle required for a menu-bar app and for
//! SMAppService Login Item registration.

use std::path::{Path, PathBuf};

pub const BUNDLE_ID: &str = "show.laowang.macleaner-bar";
pub const APP_DIR_NAME: &str = "Macleaner Bar.app";
pub const EXE_NAME: &str = "macleaner-bar";

/// Path to the installed app bundle (`~/Applications/Macleaner Bar.app`).
pub fn app_path(home: &Path) -> PathBuf {
    home.join("Applications").join(APP_DIR_NAME)
}

/// The Info.plist contents. `LSUIElement` makes it a menu-bar-only agent (no
/// Dock icon); the bundle id + executable are what SMAppService keys on.
pub fn info_plist() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>Macleaner Bar</string>
    <key>CFBundleDisplayName</key>
    <string>Macleaner Bar</string>
    <key>CFBundleIdentifier</key>
    <string>{BUNDLE_ID}</string>
    <key>CFBundleExecutable</key>
    <string>{EXE_NAME}</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleVersion</key>
    <string>{version}</string>
    <key>CFBundleShortVersionString</key>
    <string>{version}</string>
    <key>LSMinimumSystemVersion</key>
    <string>13.0</string>
    <key>LSUIElement</key>
    <true/>
</dict>
</plist>
"#,
        BUNDLE_ID = BUNDLE_ID,
        EXE_NAME = EXE_NAME,
        version = env!("CARGO_PKG_VERSION"),
    )
}

/// Build `~/Applications/Macleaner Bar.app` from `source_exe`. Returns the app
/// path. Idempotent (overwrites the executable + plist in place).
pub fn install(home: &Path, source_exe: &Path) -> anyhow::Result<PathBuf> {
    let app = app_path(home);
    let macos = app.join("Contents/MacOS");
    std::fs::create_dir_all(&macos)?;
    let exe_dst = macos.join(EXE_NAME);
    std::fs::copy(source_exe, &exe_dst)?;
    set_executable(&exe_dst)?;
    std::fs::write(app.join("Contents/Info.plist"), info_plist())?;
    Ok(app)
}

/// Remove the installed app bundle.
pub fn uninstall(home: &Path) -> anyhow::Result<()> {
    let app = app_path(home);
    if app.exists() {
        std::fs::remove_dir_all(&app)?;
    }
    Ok(())
}

fn set_executable(p: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perm = std::fs::metadata(p)?.permissions();
    perm.set_mode(0o755);
    std::fs::set_permissions(p, perm)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_plist_ac4() {
        let p = info_plist();
        assert!(p.contains("<key>LSUIElement</key>"));
        assert!(p.contains("<true/>"));
        assert!(p.contains(BUNDLE_ID));
        assert!(p.contains("<key>CFBundleExecutable</key>"));
        assert!(p.contains(EXE_NAME));
    }

    #[test]
    fn install_builds_bundle() {
        let home = std::env::temp_dir().join(format!("mbar-bundle-{}", std::process::id()));
        std::fs::create_dir_all(&home).unwrap();
        let fake_exe = home.join("fake-bin");
        std::fs::write(&fake_exe, b"#!/bin/sh\n").unwrap();
        let app = install(&home, &fake_exe).unwrap();
        assert!(app.join("Contents/MacOS/macleaner-bar").exists());
        assert!(app.join("Contents/Info.plist").exists());
        uninstall(&home).unwrap();
        assert!(!app.exists());
        std::fs::remove_dir_all(&home).ok();
    }
}
