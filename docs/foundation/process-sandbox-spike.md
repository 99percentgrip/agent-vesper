# Process-tree and sandbox conformance spike

Status: COMPLETE

## Objective

Validate the minimum process ownership, output-drain, cancellation, timeout, and
Linux isolation primitives needed before Agent Vesper designs production command,
hook, worker, diagnostic, or automation execution. Prepare executable macOS and
Windows probes without claiming results from an unavailable host.

## Classification

- Linux x86-64 process cleanup: **locally validated**.
- Linux Bubblewrap isolation: **locally validated**.
- macOS Intel/Apple Silicon process groups and Seatbelt: **CI validation pending**.
- Windows x86-64 Job Objects, rename, and ACL behavior: **CI validation pending**.
- Promotion of this disposable code: **product/architecture review pending**.

## Source behavior inspected

Confirmed at frozen source commit
`bf4d4287e2e3320aa3f09015f678e6169d520045`:

- `glm_acp/tools.py::_run_command`, lines 1975–1985, creates a new Unix
  session or Windows process group.
- The same symbol, lines 1995–2025, runs a shell string, optionally prefixes an
  OS sandbox, and assigns a Windows Job Object on the best-effort path.
- The stream collector at lines 2027–2050 drains all bytes while retaining a
  bounded head/tail representation.
- Timeout cleanup at lines 2052–2067 kills the process group, but task
  cancellation is not caught. The oracle independently reproduced two surviving
  descendants for cancellation and zero for timeout in
  `fixtures/tools/command-cancellation/result.python.json` and
  `fixtures/tools/command-timeout/result.python.json`.
- `glm_acp/os_sandbox.py::command_prefix`, lines 68–135, selects Bubblewrap,
  Seatbelt, Windows process-only containment, or fail-closed required mode.
- `glm_acp/os_sandbox.py::WindowsJob`, lines 138–190, configures
  `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`.

This spike intentionally strengthens cleanup semantics. That is a required
security invariant for Rust, not an assertion that the Python cancellation leak
is desirable parity.

## Spike design

`spikes/process-sandbox/` is an independent, disposable Cargo package. Its
fixture child provides:

- direct and silent children;
- a child/grandchild tree;
- a descendant that appears detached to the caller but stays in the owned group;
- descendants retaining stdout or stderr after their direct parent exits;
- continuous output and 8 MiB combined huge output;
- a child ignoring `SIGTERM`.

The Unix supervisor:

- assigns the direct child to a new process group;
- drains both pipes to EOF while retaining at most 64 KiB per stream;
- distinguishes exit, timeout, and cancellation terminal states;
- sends `SIGTERM`, waits 75 ms, escalates the entire group with `SIGKILL`,
  reaps the leader, and waits for pipe closure;
- cleans remaining group members even if the direct child exited first;
- makes the owner handle's `Drop` cancel the background supervision task;
- records total bytes, retained bytes, group identity, kill-to-reap duration,
  group survival, and bytes observed after termination began.

## Commands and local results

Host:

```text
Linux 7.0.0-28-generic x86_64
Bubblewrap 0.9.0
kernel.unprivileged_userns_clone=1
```

Commands:

```text
command -v bwrap
bwrap --version
unshare --user --map-root-user --pid --fork --mount-proc true
bwrap --unshare-pid --unshare-net --ro-bind / / --dev /dev --proc /proc /usr/bin/true
cargo generate-lockfile
timeout 90s cargo test --locked
pgrep -af 'fixture-child|process_conformance'
```

Final Rust result:

```text
linux_bwrap:       3 passed, 0 failed
process_conformance: 9 passed, 0 failed
doc/unit tests:    0 failed
duration:          0.72 s test execution
remaining fixture descendants: none
```

The nine process tests cover direct exit, child/grandchild timeout, explicit
cancellation, no post-cancel bytes, stdout/stderr pipe holders, ignored graceful
termination, bounded huge-output capture with complete drain, drop cleanup,
silent/detached-looking descendants, actual `getpgid` membership, and stable
`/proc/self/fd` counts.

The Linux sandbox tests proved:

- required mode returns an error when the configured Bubblewrap executable is
  unavailable;
- the sandbox has a PID namespace (`bwrap` PID 1 and test shell PID 2);
- `/proc/net/dev` contains only loopback under `--unshare-net`;
- a specifically bound temporary workspace is writable;
- a sibling path beneath a read-only root rejects writes.

An initial test incorrectly inspected host-backed `/sys/class/net`; the corrected
test uses namespace-owned `/proc/net/dev`. An interrupted early pipe-holder test
left one synthetic child, PID 65198; it was explicitly terminated and the final
bounded suite confirmed no fixture descendants remained.

## Cross-platform executable validation

The workflow `.github/workflows/foundation-spikes.yml` uses
[current standard GitHub-hosted labels](https://docs.github.com/en/actions/reference/runners/github-hosted-runners):
`ubuntu-24.04`,
`ubuntu-24.04-arm`, `macos-15-intel`, `macos-15`, and `windows-2025`.
The workflow has not run because the target has no remote or commit.

### macOS

`scripts/macos-conformance.sh` runs the Unix process tests on the real host,
requires `/usr/bin/sandbox-exec`, validates a deny-default profile, permits
workspace and temporary writes, rejects an outside write, and denies networking.
Both Intel and Apple Silicon jobs are present. Results remain **CI validation
pending**.

### Windows

`scripts/windows-conformance.ps1` P/Invokes `CreateJobObject`,
`SetInformationJobObject`, and `AssignProcessToJobObject`; enables
`KILL_ON_JOB_CLOSE`; starts a child that creates a grandchild only after Job
assignment; closes the Job handle; and asserts that both processes terminate.
It also runs file replacement/ACL probes and records the explicit refusal of
strong filesystem/network isolation. Results remain **CI validation pending**.

Windows Job Objects are process ownership, not filesystem or network isolation.
Required strong isolation must fail closed, matching the source invariant in
`os_sandbox.py:125–134`.

## Files created

- `spikes/process-sandbox/{Cargo.toml,Cargo.lock,README.md,AGENTS.md}`
- `spikes/process-sandbox/src/lib.rs`
- `spikes/process-sandbox/src/bin/fixture-child.rs`
- `spikes/process-sandbox/tests/{process_conformance.rs,linux_bwrap.rs}`
- `spikes/process-sandbox/scripts/{macos-conformance.sh,macos-workspace.sb,windows-conformance.ps1}`
- `.github/workflows/foundation-spikes.yml`

## Migration effect and unresolved issues

Local process cleanup and Linux sandbox primitives no longer block a workspace
foundation. Production design must not copy the Python cancellation gap: every
terminal path, including future/owner drop, must own and clean the process tree.
macOS, Windows, and Linux ARM64 remain external validation gates before claiming
five-target conformance. The current spike is evidence, not a production API.
