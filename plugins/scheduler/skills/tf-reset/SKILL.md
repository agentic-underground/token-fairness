---
name: tf-reset
description: >
  Re-baseline the token-fairness session. Trigger with /scheduler:tf-reset (or "reset the budget
  baseline", "re-baseline the session"). WARNS first if a cost-journal entry is still open, then runs
  `tf budget set --reset` and `tf session-boundary` and surfaces the new baseline.
metadata:
  type: mutating
  output: the warning (if any) plus the new baseline from `tf budget set --reset`
  model: inherit
---

# token-fairness — re-baseline the session

This skill MUTATES state. Run the steps in order and stop on the first failure.

1. **Check for an open cost-journal entry** (so an in-flight unit of work is not split by a reset):

   ```bash
   bash ${CLAUDE_PLUGIN_ROOT}/hooks/tf-hook.sh journal read
   ```

   If the output contains an OPEN entry for the current unit of work, **WARN the Operator** that a
   reset will re-baseline mid-unit and **ask for explicit confirmation before continuing**. (If the
   `journal` feature is not built, `tf journal` exits non-zero — treat that as "no open entry" and
   continue.)

2. **Re-baseline + cross the session boundary:**

   ```bash
   bash ${CLAUDE_PLUGIN_ROOT}/hooks/tf-hook.sh budget set --reset
   bash ${CLAUDE_PLUGIN_ROOT}/hooks/tf-hook.sh session-boundary
   ```

3. **Surface the new baseline** from the `budget set --reset` output (the `baseline_tokens` field)
   verbatim, so the Operator can confirm the re-baseline took effect.
