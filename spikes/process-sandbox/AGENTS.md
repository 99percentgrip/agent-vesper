# Process and sandbox spike instructions

This directory is a disposable conformance spike, not Agent Vesper production code.

- Keep child programs deterministic and free of external network dependencies.
- Linux tests may validate real process groups and Bubblewrap namespaces.
- macOS and Windows checks must remain explicitly platform-gated.
- Never describe an unexecuted platform check as validated.
- Preserve bounded output collection and descendant cleanup assertions.
- Output draining is cancellation-aware: `drain()` freezes the captured byte
  count the instant the supervision `CancellationToken` fires (biased so the
  cancel branch wins a tie) and keeps reading to EOF afterwards, discarding
  post-cancellation bytes. This guarantees `post_termination_output_bytes` is
  zero on cancellation regardless of bytes a child writes into the pipe buffer
  between the cancel signal and the SIGKILL that reaps its process group.
