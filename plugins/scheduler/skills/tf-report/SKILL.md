---
name: tf-report
description: >
  Show the token-fairness honesty report for this project. Trigger with /scheduler:tf-report (or
  "show the honesty report", "what has this project spent?"). Runs `tf report . --honesty` LIVE
  through the hook wrapper and surfaces the output verbatim. If the report tool exits non-zero (e.g.
  a bad path), the error is surfaced — never a fabricated report.
metadata:
  type: diagnostic
  output: the live `tf report . --honesty` text, or the tool's own error
  model: inherit
---

# token-fairness — honesty report

Run the honesty report for the current project and surface it verbatim:

```bash
bash ${CLAUDE_PLUGIN_ROOT}/hooks/tf-hook.sh report . --honesty
```

Display the output AS-IS. If the command exits non-zero, surface THAT error message and stop — do
not invent a plausible-looking report from unrelated state. The report is a read-only fold over the
honesty event ledger; its accuracy is the whole point, so a missing/empty ledger must read as
missing/empty, not as a fabricated zero-spend summary.
