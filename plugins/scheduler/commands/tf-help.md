---
description: Show the token-fairness CLI surface — runs `tf --help` live and surfaces every subcommand, then lists the slash commands that wrap them. The list is never hardcoded; it always reflects the installed binary.
---

Display the token-fairness command surface. Follow the [`tf-help` skill](../skills/tf-help/SKILL.md):
it runs `tf --help` through the hook wrapper and surfaces the binary's own output verbatim, so the
list can never drift from the installed binary. The skill then appends the available slash commands
(`/scheduler:tf-help`, `/scheduler:tf-report`, `/scheduler:tf-reset`).

The deterministic core is a static binary (`${CLAUDE_PLUGIN_ROOT}/bin/`, invoked via
`${CLAUDE_PLUGIN_ROOT}/hooks/tf-hook.sh`); this command is a read-only window onto it.
