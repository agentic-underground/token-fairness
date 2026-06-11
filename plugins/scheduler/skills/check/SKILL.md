---
name: check
description: >
  Verify TOKEN-FAIRNESS's dependencies are installed and reachable — a ✓/✗ table by tier. Trigger
  with /scheduler:check (or "check the scheduler prerequisites", "is tf installed?"). The deterministic
  core is a static binary (bin/tf) with zero runtime deps; the hooks use jq, off-peak cron uses
  flock + the claude CLI. Advisory by default (everything degrades gracefully); pass --strict to fail
  on a missing required tool. Reads the canonical manifest skills/check/requirements.tsv.
metadata:
  type: diagnostic
  output: a ✓/✗ readiness table grouped by tier
  model: inherit
---

# TOKEN-FAIRNESS readiness check

Run the probe:

```bash
bash ${CLAUDE_PLUGIN_ROOT}/skills/check/scripts/check.sh
# or, to fail the session on any missing REQUIRED tool:
bash ${CLAUDE_PLUGIN_ROOT}/skills/check/scripts/check.sh --strict
```

It reads [`requirements.tsv`](../requirements.tsv) and probes each tool, printing a ✓/✗ table grouped
by tier (required / recommended / optional) with an install hint for anything missing. The arithmetic
guard (`tf`) is a self-contained static binary — if `bin/tf-<arch>-<os>` is present (or a `tf` is on
PATH) the deterministic core is fully operational regardless of the optional tools.
