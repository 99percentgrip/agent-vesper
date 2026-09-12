# Native terminal process verification

## Purpose

Verify terminal interaction through the production TUI binary with isolated state.

## Ownership

- `settings_pty.py` owns the stdlib-only Linux/macOS Settings lifecycle smoke test.
- `update_*_fixture.sh` are immutable offline download/version fixtures for the
  Rust updater test, which runs the shipped installer only in temporary roots.
- Rust rendering and configuration unit tests remain beside their source modules.

## Local Contracts

- Use temporary HOME, workspace and global data roots plus synthetic credentials.
- Block outbound proxies, submit no provider prompt, and never install a public release or write to real user state.
- Exercise production keyboard/mouse handlers and inspect saved files and restart
  behavior. Kill and observe child processes before removing fixture directories.

## Work Guidance

- Keep terminal fixtures bounded and dependency-free; failures print the last screen.

## Verification

- Run `cargo test -p agent-vesper-tui --bin agent-vesper-tui updater_installs_verified_fixture`
  for exact-version download, checksum refusal, payload replacement and state preservation.
- Build `cargo build -p agent-vesper-tui --all-features`.
- Run `python3 apps/agent-vesper-tui/tests/settings_pty.py target/debug/agent-vesper-tui`.

## Child DOX Index

No children.
