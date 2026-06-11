#!/usr/bin/env bash
# offpeak-budget.sh — THE OVERNIGHT BUDGET CALCULATOR. The user's signature requirement, deterministic.
#
# "What time will you log in tomorrow?" → from that one answer, compute a per-window spend ceiling
# for the unattended hours so the job runs hard while the user sleeps but HANDS BACK a usable
# allowance the moment they wake. No model, no guessing — pure arithmetic over the rolling windows.
#
# The model: rolling rate-limit windows reset every --window-hours (default 5). A window that fully
# RESETS before login is the user's no longer — we may spend it down to (100 − headroom)% (default
# 85%). But the window the user INHERITS at login (the one active when they log in) must be left with
# at least --morning-reserve% free (default 60 → we stop that window at 40%). Because that window does
# not reset between our overnight use and login, leaving it ≤ (100 − reserve)% guarantees the reserve.
# The deeper into the user's peak day login falls, the more we reserve — so set a higher reserve for an
# early/peak login, a lower one for a late login. (The scheduler raises reserve when login is in peak.)
#
#   offpeak-budget.sh --now EPOCH --login EPOCH --reset EPOCH \
#       [--headroom PCT] [--morning-reserve PCT] [--window-hours N]
#
# Output (one-line JSON): the window plan + `current_headroom` to feed straight into ceiling-check.sh.
set -uo pipefail

now="" login="" reset="" headroom=15 reserve=60 wh=5
while [ $# -gt 0 ]; do
  case "$1" in
    --now) now="${2:-}"; shift 2 ;;
    --login) login="${2:-}"; shift 2 ;;
    --reset) reset="${2:-}"; shift 2 ;;
    --headroom) headroom="${2:-}"; shift 2 ;;
    --morning-reserve) reserve="${2:-}"; shift 2 ;;
    --window-hours) wh="${2:-}"; shift 2 ;;
    --now=*) now="${1#*=}"; shift ;;
    --login=*) login="${1#*=}"; shift ;;
    --reset=*) reset="${1#*=}"; shift ;;
    --headroom=*) headroom="${1#*=}"; shift ;;
    --morning-reserve=*) reserve="${1#*=}"; shift ;;
    --window-hours=*) wh="${1#*=}"; shift ;;
    -h|--help) grep '^#' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) shift ;;
  esac
done

intchk() { case "$1" in (''|*[!0-9]*) return 1 ;; (*) return 0 ;; esac; }
for v in now login reset; do intchk "${!v}" || { printf '{"error":"--%s EPOCH required"}\n' "$v"; exit 2; }; done
intchk "$headroom" || headroom=15
intchk "$reserve"  || reserve=60
intchk "$wh" && [ "$wh" -gt 0 ] || wh=5
[ "$headroom" -le 100 ] || headroom=15
[ "$reserve"  -le 100 ] || reserve=60

W=$(( wh * 3600 ))
unatt_ceiling=$(( 100 - headroom ))
login_ceiling=$(( 100 - reserve ))

# Login-window index: windows END at reset + i*W (i≥0). login in window i where
# reset+(i-1)*W < login ≤ reset+i*W.  login ≤ reset → i=0 (current window is the handover).
if [ "$login" -le "$reset" ]; then
  lwi=0
else
  diff=$(( login - reset ))
  lwi=$(( (diff + W - 1) / W ))   # ceil(diff / W)
fi

# Safety cap so a far-future login can't emit thousands of windows.
MAXW=50
truncated="false"
last_idx="$lwi"
if [ "$last_idx" -gt "$MAXW" ]; then last_idx="$MAXW"; truncated="true"; fi

windows="["
i=0
while [ "$i" -le "$last_idx" ]; do
  ends_at=$(( reset + i * W ))
  if [ "$i" -eq "$lwi" ]; then role="login"; ceil="$login_ceiling"; hr="$reserve"
  else role="unattended"; ceil="$unatt_ceiling"; hr="$headroom"; fi
  [ "$i" -gt 0 ] && windows="${windows},"
  windows="${windows}{\"index\":${i},\"ends_at\":${ends_at},\"role\":\"${role}\",\"ceiling_pct\":${ceil},\"headroom\":${hr}}"
  i=$(( i + 1 ))
done
windows="${windows}]"

# current_headroom = the headroom that applies to the window we are in RIGHT NOW (index 0).
if [ "$lwi" -eq 0 ]; then current_headroom="$reserve"; else current_headroom="$headroom"; fi
unattended_windows="$lwi"

printf '{"now":%d,"login":%d,"reset":%d,"window_hours":%d,"login_window_index":%d,"unattended_windows":%d,"current_headroom":%d,"truncated":%s,"windows":%s}\n' \
  "$now" "$login" "$reset" "$wh" "$lwi" "$unattended_windows" "$current_headroom" "$truncated" "$windows"
