---
description: Show the token-fairness honesty report for this project — runs `tf report . --honesty` live and surfaces the output verbatim. If the report tool errors (e.g. a bad path), the error is surfaced, never a fabricated report.
---

Display the honesty report for the current project. Follow the [`tf-report` skill](../skills/tf-report/SKILL.md):
it runs `tf report . --honesty` through the hook wrapper and surfaces the tool's output verbatim. If the
report command exits non-zero, the skill surfaces THAT error — it never invents a plausible-looking
report from unrelated state.

The arithmetic lives in the `tf` binary (`${CLAUDE_PLUGIN_ROOT}/bin/`, invoked via
`${CLAUDE_PLUGIN_ROOT}/hooks/tf-hook.sh`); this command is a read-only window onto it.
