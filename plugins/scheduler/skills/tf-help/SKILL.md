---
name: tf-help
description: >
  Show the token-fairness CLI surface. Trigger with /scheduler:tf-help (or "what tf commands are
  there?", "show the tf help"). Runs `tf --help` LIVE through the hook wrapper and surfaces the
  binary's own output verbatim — the subcommand list is never hardcoded, so it can never drift from
  the installed binary — then appends the wrapping slash commands.
metadata:
  type: diagnostic
  output: the live `tf --help` text plus the slash-command list
  model: inherit
---

# token-fairness — command surface

Run the binary's own help and surface it verbatim:

```bash
bash ${CLAUDE_PLUGIN_ROOT}/hooks/tf-hook.sh --help
```

Display that output AS-IS — do not summarise or re-list from memory; the binary is the source of
truth, so a newly added subcommand appears here with no edit to this skill.

Then append the slash commands that wrap it:

- `/scheduler:tf-help` — this surface listing
- `/scheduler:tf-report` — the honesty report (`tf report . --honesty`)
- `/scheduler:tf-reset` — re-baseline the session (`tf budget set --reset` + `tf session-boundary`)
