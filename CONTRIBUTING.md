# Contributing to macleaner

Thanks for your interest. macleaner is a small, safety-first macOS utility — the
bar for any change touching a destructive path (the cleaner's deletions, the
menu-bar app's process-quitting) is high.

## Workspace layout

```
macleaner/         Cargo workspace
├── daemon/        crate "macleaner"      → the daily disk-cleaner LaunchAgent
└── bar/           crate "macleaner-bar"  → the native menu-bar companion
```

The two are loosely coupled: the bar shells out to `~/bin/macleaner`; it does
not import the daemon.

## Local checks (must pass before a PR)

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release --workspace
```

CI runs exactly these on macOS for every PR.

## Safety rules (non-negotiable)

- **Cleaner deletions** stay on an allowlist of roots, are age-gated, never
  follow symlinks off-target, and never touch anything under `/Volumes`. Any new
  cleaner must add tests proving it deletes only what it should and respects
  dry-run.
- **The memory view's quit path** re-resolves every pid and re-applies
  `is_killable` — it must never be able to target `pid <= 1`, its own process,
  or a critical system process, and must never run `purge` or similar snake-oil.
- New destructive behavior needs a unit test for the safety guard AND should
  default to a dry-run / preview where feasible.

## Style

Idiomatic Rust, rustfmt-clean, clippy-clean. Match the surrounding code's
comment density and naming.
