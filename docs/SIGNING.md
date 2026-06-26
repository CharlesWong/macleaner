# Code signing & notarization

**Release binaries are signed with a Developer ID and notarized by Apple**
(since v0.1.0), so a browser-downloaded `.tar.gz` runs without a Gatekeeper
quarantine prompt. Signing/notarization uses the maintainer's Apple Developer
account ($99/year + a "Developer ID Application" certificate) and **runs on the
maintainer's Mac** — the private key never enters CI.

## Release flow

`.github/workflows/release.yml` builds the universal binaries on a `v*` tag.
After it publishes, the maintainer runs [`scripts/notarize.sh`](../scripts/notarize.sh),
which signs + notarizes those binaries and replaces the release assets in place:

```sh
scripts/notarize.sh v0.1.0
```

One-time setup for that script: a "Developer ID Application" cert in the login
keychain, and a stored notarytool credential profile:
```sh
xcrun notarytool store-credentials macleaner-notary \
  --key AuthKey_XXXXXXXXXX.p8 --key-id XXXXXXXXXX --issuer <issuer-uuid>
```

> Bare CLI binaries can't be *stapled*, so Gatekeeper checks their notarization
> ticket online. A future menu-bar `.app`-bundle distribution can be stapled for
> fully-offline verification (see the recipe below).

## Building from source (unsigned)

A binary you `cargo build` yourself is unsigned, but terminal-launched binaries
you built locally aren't Gatekeeper-blocked. If you ever need to clear a
quarantine flag on a file you downloaded:
```sh
xattr -dr com.apple.quarantine macleaner macleaner-bar
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
