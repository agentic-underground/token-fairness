#!/usr/bin/env bash
# verify-prereqs.sh — the single-plugin marketplace gate for token-fairness (plan §3.6).
#
# The idea-to-production verify-prereqs asserts byte-parity of check.sh / SOUL.md / inject-soul.sh
# ACROSS ≥2 plugins — which fails by construction with one plugin. This variant drops those
# multi-copy parity checks and replaces them with the invariants that matter for a binary-backed,
# single-plugin marketplace:
#   • plugin.json / marketplace.json well-formed and version-pinned to the Cargo workspace
#     (the bin/tf ↔ build-target parity that the multi-copy parity check used to provide);
#   • every hooks.json command is a `bash …` shim — NOT the ELF directly — so verify-prereqs
#     Check L (`bash -n` + smoke) passes (the documented carve-out for binary-backed hooks);
#   • a per-arch binary is present, executable, and smoke-runs deterministically;
#   • NO .mcp.json ships (review W5) — MCP Checks C/K pass vacuously, stated explicitly.
#
# Exits non-zero on any failure. Needs bash + jq (for JSON validation).
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PLUGIN="$ROOT/plugins/scheduler"
green=$'\033[32m'; red=$'\033[31m'; dim=$'\033[2m'; rst=$'\033[0m'
[ -t 1 ] || { green=""; red=""; dim=""; rst=""; }
fail=0
ok()   { printf '  %s✓%s %s\n' "$green" "$rst" "$1"; }
bad()  { printf '  %s✗%s %s\n' "$red" "$rst" "$1"; fail=$((fail+1)); }

command -v jq >/dev/null 2>&1 || { echo "verify-prereqs: jq required"; exit 2; }

printf '\n== A. manifests ==\n'
PJ="$PLUGIN/.claude-plugin/plugin.json"; MJ="$ROOT/.claude-plugin/marketplace.json"
jq -e . "$PJ" >/dev/null 2>&1 && ok "plugin.json is valid JSON" || bad "plugin.json invalid"
jq -e . "$MJ" >/dev/null 2>&1 && ok "marketplace.json is valid JSON" || bad "marketplace.json invalid"
[ "$(jq -r '.name' "$PJ" 2>/dev/null)" = "scheduler" ] && ok "plugin name = scheduler" || bad "plugin name mismatch"
[ "$(jq -r '.plugins[0].source' "$MJ" 2>/dev/null)" = "./plugins/scheduler" ] && ok "marketplace points at ./plugins/scheduler" || bad "marketplace source mismatch"

printf '\n== B. version pin (bin ↔ build-target parity) ==\n'
PV="$(jq -r '.version' "$PJ" 2>/dev/null)"
MV="$(jq -r '.plugins[0].version' "$MJ" 2>/dev/null)"
CV="$(grep -m1 '^version' "$ROOT/Cargo.toml" 2>/dev/null | sed -E 's/.*"([^"]+)".*/\1/')"
[ -n "$CV" ] || CV="$(awk -F'"' '/^version/{print $2; exit}' "$ROOT/Cargo.toml")"
if [ "$PV" = "$MV" ] && [ "$PV" = "$CV" ]; then ok "version $PV pinned across plugin/marketplace/Cargo"
else bad "version drift: plugin=$PV marketplace=$MV cargo=$CV"; fi

printf '\n== C. hooks: bash-shim invariant + Check L (bash -n) ==\n'
HJ="$PLUGIN/hooks/hooks.json"
jq -e . "$HJ" >/dev/null 2>&1 && ok "hooks.json is valid JSON" || bad "hooks.json invalid"
# Every hook command must start with `bash ` (the shim), never reference bin/ directly.
cmds="$(jq -r '.hooks[][].hooks[].command' "$HJ" 2>/dev/null)"
if printf '%s\n' "$cmds" | grep -qvE '^bash '; then bad "a hook command is not a 'bash …' shim"; else ok "all hook commands are bash shims"; fi
if printf '%s\n' "$cmds" | grep -q '/bin/tf-'; then bad "a hook invokes the ELF binary directly (fails bash -n)"; else ok "no hook invokes the ELF directly (Check L carve-out honoured)"; fi
# bash -n every shipped hook/skill script.
script_ok=1
while IFS= read -r f; do
  bash -n "$f" 2>/dev/null || { bad "bash -n failed: ${f#$ROOT/}"; script_ok=0; }
done < <(find "$PLUGIN" -name '*.sh' -type f)
[ "$script_ok" = 1 ] && ok "bash -n clean on every shipped .sh"

printf '\n== D. binary resolution: local smoke OR lazy-download wiring ==\n'
arch="$(uname -m)"; os="$(uname -s | tr 'A-Z' 'a-z')"; case "$os" in linux*) os=linux;; darwin*) os=darwin;; esac
BIN="$PLUGIN/bin/tf-${arch}-${os}"
SHIM="$PLUGIN/hooks/tf-hook.sh"
if [ -x "$BIN" ]; then
  # A locally built/cached binary is present (the CI build path, and any dev tree) — smoke it.
  ok "bin/tf-${arch}-${os} present + executable"
  out="$("$BIN" offpeak-window --now 1700000000 --tz-offset-min 0 2>/dev/null)"
  echo "$out" | jq -e '.in_offpeak != null' >/dev/null 2>&1 && ok "binary smoke-runs (deterministic offpeak-window)" || bad "binary smoke failed"
else
  # No committed binary (the shipped end-user tree): the shim MUST lazy-download it from a release.
  ok "no committed binary for ${arch}-${os} — verifying lazy-download wiring instead"
  grep -q 'releases/download' "$SHIM" && ok "shim downloads the per-arch asset from a release" || bad "shim has no release-download path"
  grep -qE 'sha256|shasum|SHA256SUMS' "$SHIM" && ok "shim checksum-verifies the download" || bad "shim does not verify the download checksum"
  [ -f "$PLUGIN/bin/README.md" ] && ok "bin/README.md documents lazy-download" || bad "bin/README.md missing (explain the empty bin/)"
fi

printf '\n== E. MCP surface (review W5) ==\n'
if [ -e "$PLUGIN/.mcp.json" ] || [ -e "$ROOT/.mcp.json" ]; then bad ".mcp.json present — this plugin ships none"; else ok "no .mcp.json (MCP Checks C/K pass vacuously — stated explicitly)"; fi

printf '\n========================================\n'
if [ "$fail" -eq 0 ]; then
  printf '%sverify-prereqs GREEN — single-plugin marketplace is sound. We are go for launch.%s\n' "$green" "$rst"
  exit 0
else
  printf '%sverify-prereqs: %d check(s) FAILED.%s\n' "$red" "$fail" "$rst"
  exit 1
fi
