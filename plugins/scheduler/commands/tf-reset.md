---
description: Re-baseline the token-fairness session — runs `tf budget set --reset` then `tf session-boundary`, confirming the new baseline. Warns first if a cost-journal entry is still open, so an in-flight unit of work is not silently re-baselined.
---

Re-baseline the current session. Follow the [`tf-reset` skill](../skills/tf-reset/SKILL.md): it first
checks `tf journal read` for an OPEN entry and WARNS the Operator before doing anything if one is
present (a reset across an open journal entry would split that unit's spend). With confirmation (or no
open entry), it runs `tf budget set --reset` then `tf session-boundary` and surfaces the new baseline.

The state writes go through the `tf` binary (`${CLAUDE_PLUGIN_ROOT}/bin/`, invoked via
`${CLAUDE_PLUGIN_ROOT}/hooks/tf-hook.sh`); this command is the discipline around them.
