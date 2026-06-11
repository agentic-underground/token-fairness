#!/usr/bin/env bash
# tf-hook.sh — the bash shim every hook command invokes. A compiled binary cannot be the
# hook command directly: verify-prereqs Check L runs `bash -n` + a smoke-exec on each
# hooks.json command, and an ELF fails `bash -n`. This script DOES parse as bash, resolves
# the correct per-arch `tf` binary (review C2/§3.3), and exec's it — forwarding stdin, args,
# and exit code unchanged. Cron/hook env is minimal, so we resolve absolute paths here.
#
#   bash tf-hook.sh <verb> [args…]
set -uo pipefail

ROOT="${CLAUDE_PLUGIN_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
arch="$(uname -m 2>/dev/null || echo unknown)"
os="$(uname -s 2>/dev/null | tr 'A-Z' 'a-z' || echo unknown)"
case "$os" in linux*) os=linux ;; darwin*) os=darwin ;; esac
bin="${ROOT}/bin/tf-${arch}-${os}"

# Resolver: prefer the shipped per-arch binary; fall back to a `tf` on PATH (cargo-install
# or a dev build). Either way the determinism is identical.
if [ -x "$bin" ]; then exec "$bin" "$@"; fi
if command -v tf >/dev/null 2>&1; then exec tf "$@"; fi

# No binary for this platform and none on PATH. Hooks are wrapped in `|| true`, so failing
# soft (exit 0, no stdout) is the safe default — it never blocks the session.
echo "tf-hook: no tf binary for ${arch}-${os} and no 'tf' on PATH — see plugins/scheduler/bin/" >&2
exit 0
