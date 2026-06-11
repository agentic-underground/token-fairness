#!/usr/bin/env bash
# check.sh — probe TOKEN-FAIRNESS's dependencies from the canonical requirements.tsv and print a
# ✓/✗ table grouped by tier. Advisory by default; --strict exits non-zero on a missing required tool.
set -uo pipefail

ROOT="${CLAUDE_PLUGIN_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)}"
TSV="${ROOT}/skills/check/requirements.tsv"
strict=0; [ "${1:-}" = "--strict" ] && strict=1
[ -r "$TSV" ] || { echo "check: requirements.tsv not found at $TSV" >&2; exit 2; }

green=$'\033[32m'; red=$'\033[31m'; dim=$'\033[2m'; rst=$'\033[0m'
[ -t 1 ] || { green=""; red=""; dim=""; rst=""; }

miss_required=0
printf 'TOKEN-FAIRNESS readiness\n\n'
while IFS=$'\t' read -r name probe tier hint; do
  case "$name" in ''|\#*) continue ;; esac
  if eval "$probe" >/dev/null 2>&1; then
    printf '  %s✓%s %-9s %s%s%s\n' "$green" "$rst" "$name" "$dim" "$tier" "$rst"
  else
    printf '  %s✗%s %-9s %s%s — %s%s\n' "$red" "$rst" "$name" "$dim" "$tier" "$hint" "$rst"
    [ "$tier" = "required" ] && miss_required=$((miss_required+1))
  fi
done < "$TSV"

echo
if [ "$miss_required" -gt 0 ]; then
  printf '%s%d required tool(s) missing.%s\n' "$red" "$miss_required" "$rst"
  [ "$strict" = 1 ] && exit 1
else
  printf '%sCore ready — the tf guard is operational. Light is green, trap is clean.%s\n' "$green" "$rst"
fi
exit 0
