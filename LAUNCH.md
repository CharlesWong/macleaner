# macleaner — launch readiness

**Goal:** take macleaner from "works on my machine" to a shippable open-source
macOS release: continuously tested, downloadable as prebuilt binaries, honestly
documented (including the signing reality), and trustworthy by design.

## Launch criteria

1. **Continuous integration.** `.github/workflows/ci.yml` runs
   `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`, and a release
   build of the whole workspace on macOS (arm64) for every push and PR. The
   repo carries a CI badge.

2. **Prebuilt releases.** `.github/workflows/release.yml` builds **universal**
   binaries (Apple Silicon + Intel via `lipo`) on a `v*` tag and publishes a
   GitHub Release with a `.tar.gz` and a SHA-256 checksum. `scripts/package.sh`
   reproduces this locally. So a user can install without a Rust toolchain.

3. **Code-signing / notarization — documented, not yet done.** `docs/SIGNING.md`
   states the binaries are currently unsigned/un-notarized, explains the
   Gatekeeper/quarantine consequence and the `xattr -dr com.apple.quarantine`
   workaround, and gives the exact `codesign` → `notarytool` → `stapler` recipe.
   This is the one launch step that **requires the maintainer's Apple Developer
   account** ($99/yr + Developer ID cert) and cannot be automated without it.

4. **Install + Gatekeeper guidance.** The README documents both install paths —
   download a release, or build from source — plus the `macleaner install` /
   `macleaner-bar install` steps, the Full Disk Access note (only if a cleaner
   reaches a protected folder), and the quarantine workaround for the unsigned
   app.

5. **License + contributing.** MIT `LICENSE` at the root; `CONTRIBUTING.md` with
   the local-checks command and the non-negotiable safety rules; a bug-report
   issue template.

6. **Safety / trust posture (the differentiator).** The cleaner only prunes an
   allowlist of regenerating junk, age-gates deletions, never follows symlinks
   off-target, never touches `/Volumes`, and previews on first run. The
   memory-relief view never runs `purge` and re-validates every pid before a
   graceful SIGTERM. This is stated plainly in the README and enforced by tests
   and CI — the trust story is a feature, not a footnote.

## Explicitly out of scope for v0.1.0

- Apple notarization (blocked on the Developer account — scaffolded + documented).
- A Homebrew tap / cask (a fast follow once releases exist).
- New features from the competitive research (duplicate finder, large-file
  finder, app uninstaller) — those are a separate roadmap.

## Definition of done

CI green on `main`; a tagged `v0.1.0` GitHub Release with universal binaries +
checksum; README/​docs cover install, Gatekeeper, and safety; LICENSE present.
