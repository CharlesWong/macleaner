#!/usr/bin/env bash
# Sign + notarize a published macleaner release's universal binaries, then
# replace the release assets in place with the signed/notarized tarball.
#
# Run this AFTER the CI release workflow (.github/workflows/release.yml) has
# built and published the (unsigned) universal binaries for a tag — it keeps the
# Developer ID private key on your Mac instead of in CI secrets.
#
# One-time prerequisites:
#   - A "Developer ID Application" certificate in your login keychain
#     (verify: security find-identity -v -p codesigning).
#   - A stored notarytool credential profile:
#       xcrun notarytool store-credentials macleaner-notary \
#         --key AuthKey_XXXXXXXXXX.p8 --key-id XXXXXXXXXX --issuer <issuer-uuid>
#   - gh authenticated with write access to the repo.
#
# Usage: scripts/notarize.sh vX.Y.Z
#
# Env overrides:
#   SIGN_IDENTITY  codesign identity (default: "Developer ID Application" — set
#                  to your full "Developer ID Application: Name (TEAMID)" if the
#                  keychain holds more than one).
#   NOTARY_PROFILE notarytool keychain profile name (default: macleaner-notary).
#   MACLEANER_REPO owner/name (default: CharlesWong/macleaner).
set -euo pipefail

VER="${1:?usage: scripts/notarize.sh vX.Y.Z}"
REPO="${MACLEANER_REPO:-CharlesWong/macleaner}"
IDENTITY="${SIGN_IDENTITY:-Developer ID Application}"
PROFILE="${NOTARY_PROFILE:-macleaner-notary}"
TAR="macleaner-${VER}-universal-macos.tar.gz"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
cd "$work"

echo "→ downloading $TAR from $REPO @ $VER"
gh release download "$VER" --repo "$REPO" --pattern "$TAR" --output "$TAR"
tar -xzf "$TAR"

echo "→ signing (Developer ID + hardened runtime + secure timestamp)"
codesign --force --options runtime --timestamp --sign "$IDENTITY" macleaner macleaner-bar
codesign --verify --strict macleaner macleaner-bar

echo "→ notarizing (waits for Apple's verdict; the notary service can take 10–40 min)"
mkdir notar-stage
cp macleaner macleaner-bar notar-stage/
/usr/bin/ditto -c -k --keepParent notar-stage notarize.zip   # zip both signed binaries
xcrun notarytool submit notarize.zip --keychain-profile "$PROFILE" --wait
# NOTE: bare CLI binaries cannot be stapled — Gatekeeper checks the notarization
# ticket online. (A future .app-bundle distribution can be stapled for offline.)

echo "→ repackaging signed binaries + checksum"
rm -f "$TAR" "${TAR}.sha256" notarize.zip
tar -czf "$TAR" macleaner macleaner-bar
shasum -a 256 "$TAR" > "${TAR}.sha256"

echo "→ replacing the release assets in place (no new tag, no CI rebuild)"
gh release upload "$VER" "$TAR" "${TAR}.sha256" --repo "$REPO" --clobber

echo "✓ $VER is signed + notarized. Verify on a clean Mac:"
echo "    codesign --verify --strict macleaner macleaner-bar"
echo "    shasum -a 256 -c ${TAR}.sha256"
