# Code signing & notarization

The released binaries are currently **unsigned and un-notarized**. They run
fine, but macOS Gatekeeper treats them as "unidentified developer" — so a user
who downloads them via a browser gets a quarantine prompt on first launch.

This is the single biggest launch-readiness gap, and it's the one piece that
**requires the maintainer's Apple Developer account** ($99/year + a "Developer
ID Application" certificate). It can't be automated without those credentials.

## What the user sees today (unsigned)

- **Binaries copied/installed locally** (e.g. via `scp`, or run from the cloned
  repo): run normally — terminal-launched binaries aren't Gatekeeper-blocked.
- **Downloaded via browser** (the `.tar.gz` from a GitHub Release): the files
  carry the `com.apple.quarantine` xattr, so the menu-bar `.app` is blocked on
  first launch. Workaround:
  ```sh
  xattr -dr com.apple.quarantine macleaner macleaner-bar
  # or, for the installed app:
  xattr -dr com.apple.quarantine ~/Applications/"Macleaner Bar.app"
  ```

## How to sign + notarize (once you have a Developer ID)

1. Create a "Developer ID Application" certificate in your Apple Developer
   account and install it in the login keychain.
2. Sign the raw binaries with the hardened runtime:
   ```sh
   codesign --force --options runtime --timestamp \
     --sign "Developer ID Application: <Your Name> (<TEAMID>)" \
     macleaner macleaner-bar
   ```
3. Build the menu-bar `.app` bundle (the installer copies the signed
   `macleaner-bar` into `~/Applications/Macleaner Bar.app`), then sign the
   bundle itself — the `.app` does not exist until this step:
   ```sh
   ./macleaner-bar install
   codesign --force --options runtime --timestamp \
     --sign "Developer ID Application: <Your Name> (<TEAMID>)" \
     ~/Applications/"Macleaner Bar.app"
   ```
4. Notarize via `notarytool` (store credentials once with
   `xcrun notarytool store-credentials`). Zip the signed bundle — the `.app` is
   the thing Gatekeeper checks on launch:
   ```sh
   ditto -c -k --keepParent ~/Applications/"Macleaner Bar.app" macleaner-notarize.zip
   xcrun notarytool submit macleaner-notarize.zip \
     --keychain-profile "macleaner-notary" --wait
   ```
5. **Staple** the ticket so it verifies offline:
   ```sh
   xcrun stapler staple ~/Applications/"Macleaner Bar.app"
   ```

## Wiring it into CI

When a `MACOS_CERTIFICATE` (base64 .p12), `MACOS_CERTIFICATE_PWD`, and an
`APPLE_NOTARY_*` credential set are added as **encrypted repository credentials**
(GitHub repo → Settings → *Actions*), the `release.yml` workflow can import the
cert into a temporary keychain and run the `codesign` → `notarytool` → `stapler`
steps above before uploading. Until those credentials exist, releases stay
unsigned and the Gatekeeper note above applies.
