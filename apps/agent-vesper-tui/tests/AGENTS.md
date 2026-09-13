# Native terminal process verification

## Purpose

Verify terminal interaction through the production TUI binary with isolated state.

## Ownership

- `voice_chunks.py` verifies ten-minute production PCM slicing and resume order
  with real numpy and controlled inference.
- `voice_pty.py` exercises the real voice footer/worker with microphone-free
  `voice_recorder_fixture.py` and `voice_model_fixture.py`. An isolated Python
  with numpy is passed explicitly; no microphone/provider/package setup is used.
  It verifies ten-minute PCM, >90-second progressing inference, warm reuse,
  retry/discard, recorder/disk failures, editable input and normal-exit cleanup.

- `dependency_setup_pty.py` checks real setup consent/decline/failure/retry with
  an explicit missing engine override, structurally preventing package installation.

- `settings_pty.py` owns the stdlib-only Linux/macOS Settings lifecycle smoke test.
- `skill_routing_pty.py` reuses its isolated terminal driver for Skills draft,
  discard, keep-editing, save, restart, model-assistance opt-in and unchanged-library checks.
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
