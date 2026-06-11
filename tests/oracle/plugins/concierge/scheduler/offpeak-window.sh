#!/usr/bin/env bash
# offpeak-window.sh — L3 OFF-PEAK CLOCK. Pure, deterministic time arithmetic.
#
# Answers "are we off-peak right now, and how do the rolling-window resets line up with it?"
# so the scheduler knows when unattended work is allowed and when a reset lands in the quiet hours.
# Off-peak defaults to 22:00–08:00 (the user's stated window); it wraps midnight, handled explicitly.
#
#   offpeak-window.sh --now EPOCH [--start HH:MM] [--end HH:MM] [--reset EPOCH] [--tz-offset-min M]
#
# --tz-offset-min is local-minus-UTC in minutes (e.g. -420 for UTC-7). Default: the machine's
# current offset (via date). Working in epoch+offset keeps it DST-correct for the instant given
# and fully testable (tests pass an explicit offset).
#
# Output (one-line JSON): {in_offpeak, minutes_to_offpeak, minutes_to_reset, reset_in_window, local_hhmm}
set -uo pipefail

now="" start="22:00" end="08:00" reset="" tzoff=""
while [ $# -gt 0 ]; do
  case "$1" in
    --now) now="${2:-}"; shift 2 ;;
    --start) start="${2:-}"; shift 2 ;;
    --end) end="${2:-}"; shift 2 ;;
    --reset) reset="${2:-}"; shift 2 ;;
    --tz-offset-min) tzoff="${2:-}"; shift 2 ;;
    --now=*) now="${1#*=}"; shift ;;
    --start=*) start="${1#*=}"; shift ;;
    --end=*) end="${1#*=}"; shift ;;
    --reset=*) reset="${1#*=}"; shift ;;
    --tz-offset-min=*) tzoff="${1#*=}"; shift ;;
    -h|--help) grep '^#' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) shift ;;
  esac
done

case "$now" in (''|*[!0-9]*) echo '{"error":"--now EPOCH required"}'; exit 2 ;; esac

# Default tz offset from the machine if not supplied. date +%z → ±HHMM.
if [ -z "$tzoff" ]; then
  z="$(date +%z 2>/dev/null)"   # e.g. -0700
  case "$z" in
    [+-][0-9][0-9][0-9][0-9])
      sign="${z:0:1}"; h="${z:1:2}"; m="${z:3:2}"
      tzoff=$(( 10#$h*60 + 10#$m )); [ "$sign" = "-" ] && tzoff=$(( -tzoff )) ;;
    *) tzoff=0 ;;
  esac
fi
case "$tzoff" in (''|-) tzoff=0 ;; (*[!0-9-]*) tzoff=0 ;; esac

# HH:MM → seconds-of-day; returns empty on malformed.
hm_to_sec() {
  case "$1" in
    [0-9][0-9]:[0-9][0-9]|[0-9]:[0-9][0-9])
      local h="${1%%:*}" m="${1##*:}"
      printf '%d' $(( 10#$h*3600 + 10#$m*60 )) ;;
    *) printf '' ;;
  esac
}
start_sec="$(hm_to_sec "$start")"; [ -n "$start_sec" ] || start_sec=79200   # 22:00
end_sec="$(hm_to_sec "$end")";     [ -n "$end_sec" ]   || end_sec=28800     # 08:00

# local seconds-of-day for an epoch under tzoff.
local_sod() { local e=$(( $1 + tzoff*60 )); local s=$(( e % 86400 )); [ "$s" -lt 0 ] && s=$(( s + 86400 )); printf '%d' "$s"; }

# membership in the (possibly midnight-wrapping) off-peak window.
in_window() {  # <sod> → 0 if inside
  local s="$1"
  if [ "$start_sec" -le "$end_sec" ]; then
    [ "$s" -ge "$start_sec" ] && [ "$s" -lt "$end_sec" ]
  else  # wraps midnight: inside if at/after start OR before end
    [ "$s" -ge "$start_sec" ] || [ "$s" -lt "$end_sec" ]
  fi
}

now_sod="$(local_sod "$now")"
if in_window "$now_sod"; then in_offpeak="true"; else in_offpeak="false"; fi

# minutes until off-peak begins (0 if already in it).
if [ "$in_offpeak" = "true" ]; then
  mins_to_offpeak=0
else
  delta=$(( (start_sec - now_sod + 86400) % 86400 ))
  mins_to_offpeak=$(( delta / 60 ))
fi

# reset alignment.
if [ -n "$reset" ] && [ -z "${reset//[0-9]/}" ]; then
  mins_to_reset=$(( (reset - now) / 60 ))
  reset_sod="$(local_sod "$reset")"
  if in_window "$reset_sod"; then reset_in_window="true"; else reset_in_window="false"; fi
else
  mins_to_reset="null"; reset_in_window="null"
fi

hh=$(( now_sod / 3600 )); mm=$(( (now_sod % 3600) / 60 ))
printf '{"in_offpeak":%s,"minutes_to_offpeak":%d,"minutes_to_reset":%s,"reset_in_window":%s,"local_hhmm":"%02d:%02d"}\n' \
  "$in_offpeak" "$mins_to_offpeak" "$mins_to_reset" "$reset_in_window" "$hh" "$mm"
