
      # 10 lessons from operating a headless fleet

      12 min read & updated 2026-08

      1. **Bind nothing.** If a component can run without a
   listener, run it without one. Anonymous pipes replaced our
   debug-port fleet overnight.
2. **Fail closed.** A failed probe is a refusal, not a
   fallback to uncontained mode.
3. **Cap everything.** Every channel carries a byte bound.
   Unbounded output is an outage waiting for a schedule.
4. **Reap your children.** Parent-death signals chained into
   the PID namespace mean teardown is total even on crash.
5. **Never trust page content.** Everything scraped is data
   with a provenance header, never instructions.
6. **Index stability matters.** Element indices that shuffle
   between steps make agents click the wrong things.
7. **Redact before you serialize.** Password and payment
   fields never enter the model-visible map.
8. **Bound the DOM.** A hundred-thousand-node page is a perf
   bug, not a content feature.
9. **Keep the fallback ladder short.** One escalation, then
   an honest error.
10. **Log the denial reason.** Typed denials turn crawls into
   debuggable programs.

      ## Postscript

      None of these required new technology. They required deciding
 that honest failure beats silent degradation.
