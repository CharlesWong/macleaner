#!/usr/bin/env bash
# Build universal (arm64 + x86_64) macOS binaries locally and package them,
# mirroring what .github/workflows/release.yml does in CI.
#
# Usage: scripts/package.sh [version]   (version defaults to the daemon crate's)
set -euo pipefail
cd "$(dirname "$0")/.."

VER="${1:-$(grep -m1 '^version' daemon/Cargo.toml | sed 's/.*"\(.*\)".*/\1/')}"
# Normalize to a leading 'v' so the artifact name always matches the CI release
# (release.yml uses the v-prefixed tag) — a bare "0.1.0" and "v0.1.0" must not
# produce two differently-named, non-interchangeable tarballs.
[[ "$VER" =~ ^v ]] || VER="v$VER"
# VER flows into output filenames; reject anything but a tag-shaped string so a
# stray '/' or shell metacharacter can't produce a broken or surprising path.
if ! [[ "$VER" =~ ^v[0-9]+\.[0-9]+\.[0-9]+([.-][0-9A-Za-z.-]+)?$ ]]; then
  echo "error: version '$VER' is not a valid release version (e.g. v0.1.0)" >&2
  exit 1
fi
echo "Packaging macleaner ${VER}"

# Cross-compiling both slices needs rustup-managed std targets. A Homebrew/
# distro rustc ships only the host arch, so fail fast with a clear message
# instead of a confusing "can't find crate for std" later.
if ! command -v rustup >/dev/null 2>&1; then
  echo "error: rustup is required to build the universal binary (cross-compiling" >&2
  echo "       x86_64 + arm64). This rustc ($(command -v rustc)) can't add targets." >&2
  echo "       Install rustup, or let .github/workflows/release.yml build the release." >&2
  exit 1
fi
rustup target add aarch64-apple-darwin x86_64-apple-darwin

# Gate the package on the same checks CI runs, in the same order: fmt, clippy,
# tests — so a locally-built tarball can't contain code CI would have rejected.
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked

cargo build --release --workspace --locked --target aarch64-apple-darwin
cargo build --release --workspace --locked --target x86_64-apple-darwin

rm -rf dist && mkdir -p dist   # start clean so stale tarballs don't accumulate
# The workspace's two [[bin]] targets. Note: tests run on the host arch only;
# the x86_64 slice is cross-built but not unit-tested (no Rosetta test run).
for b in macleaner macleaner-bar; do
  for arch in aarch64-apple-darwin x86_64-apple-darwin; do
    if [ ! -f "target/$arch/release/$b" ]; then
      echo "error: expected binary target/$arch/release/$b not found — was '$b' renamed in Cargo.toml?" >&2
      exit 1
    fi
  done
  lipo -create -output "dist/$b" \
    "target/aarch64-apple-darwin/release/$b" \
    "target/x86_64-apple-darwin/release/$b"
  lipo -info "dist/$b"
done

# Smoke-test (mirrors CI): run the daemon's native slice (no UI), plus the
# x86_64 slice if this host can (Rosetta or native Intel), and verify — without
# executing — that the menu-bar binary carries both arches (it starts a
# status-item UI, which must not run headless).
./dist/macleaner --version
if arch -x86_64 true 2>/dev/null; then
  arch -x86_64 ./dist/macleaner --version
else
  echo "note: skipping x86_64 smoke-test (no Rosetta / not an Intel host)"
fi
lipo dist/macleaner-bar -verify_arch arm64 x86_64

cd dist
tar -czf "macleaner-${VER}-universal-macos.tar.gz" macleaner macleaner-bar
shasum -a 256 "macleaner-${VER}-universal-macos.tar.gz" \
  > "macleaner-${VER}-universal-macos.tar.gz.sha256"
echo "→ dist/macleaner-${VER}-universal-macos.tar.gz"
