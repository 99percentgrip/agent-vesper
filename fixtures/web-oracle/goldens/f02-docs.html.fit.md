
    # Policy Configuration Reference

      ## Overview

      The policy file is TOML. It declares rules, a mode, and optional
 metadata. Rules are evaluated in declaration order and the first
 match wins.

      ## Options

      mode
  One of allowlist or denylist. Defaults to denylist.
rules
  An array of rule tables. Each has pattern and decision.
version
  Schema version integer, currently 1.

      ```
[policy]
mode = "denylist"
version = 1

[[policy.rules]]
pattern = "fork-bomb"
decision = "deny"
```

      ## Precedence

      Denial outranks approval which outranks allowance. A rule that
 denies is absolute: no later rule, mode, or operator override can
 permit the operation.

      > Denial is absolute. Everything else is a speed bump.

      Examples>
      Examples below show minimal complete configurations for the two
 modes.

      - A denylist with one rule (shown above).
- An allowlist that denies everything not matched:
  1. Set `mode = "allowlist"`.
    2. List the allowed patterns.
