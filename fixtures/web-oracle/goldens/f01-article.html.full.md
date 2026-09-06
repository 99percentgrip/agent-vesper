
      # Sandboxing Agentic Browser Automation

      By the Platform Team & September 2026

      Agentic browser automation needs a hard boundary between the
 agent process and the host. This article walks through a design where
 the browser is spawned with its debugging channel wired to inherited
 anonymous pipes instead of a TCP listener, so the channel is
 unreachable from outside the process tree.

      ## Why pipes beat sockets

      A TCP debug listener, even on loopback, is a socket others can
 probe. Anonymous pipes exist only as file descriptors; there is no
 listening socket to attack and no port to collide with. The kernel
 enforces that only the parent and child hold descriptors to the
 same pipe.

      ### FD layout

      The child reads commands on FD 3 and writes events on FD 4.
 The parent duplicates its ends before exec so nothing else inherits
 them. Teardown is closing the write end: the child observes EOF and
 exits, and the supervisor reaps the process.

      ```
spawn --remote-debugging-pipe --no-sandbox --disable-gpu
```

      ### Fail-closed capability reporting

      Any probe that cannot complete reports the capability as
 unavailable rather than guessing. The orchestrator then refuses the
 isolated operation instead of silently degrading to an uncontained
 run.

      | Host capability | Probe result | Reported |
| --- | --- | --- |
| Namespaces allowed | probe ok | Available |
| UID map write blocked | probe fail | Unavailable |
| Docker daemon up | version ok |  |
| Docker daemon down | version fail | Unavailable |

        The debugging channel as the only boundary crossing.

      ## Conclusion

      Pipe-based debugging channels, paired with fail-closed capability
 reporting, give agentic browser sessions a containment story that
 survives missing privileges without pretending they exist.
