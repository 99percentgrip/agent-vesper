# Process and sandbox spike instructions

This directory is a disposable conformance spike, not Agent Vesper production code.

- Keep child programs deterministic and free of external network dependencies.
- Linux tests may validate real process groups and Bubblewrap namespaces.
- macOS and Windows checks must remain explicitly platform-gated.
- Never describe an unexecuted platform check as validated.
- Preserve bounded output collection and descendant cleanup assertions.
