#!/usr/bin/env bash
# Tests for offpeak-window.sh (clock math, midnight wrap, tz, reset alignment) and
# offpeak-budget.sh (the overnight per-window budget plan that reserves the morning).
set -uo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
. "${HERE}/lib/harness.sh"
OW="${HERE}/../offpeak-window.sh"
OB="${HERE}/../offpeak-budget.sh"
jget() { printf '%s' "$1" | jq -r "$2"; }

# ---------- offpeak-window.sh (tz 0; epochs are seconds-of-day on day 0) ----------
test_case "02:00 is inside the 22:00–08:00 window"
out="$(bash "$OW" --now 7200 --tz-offset-min 0)"
assert_eq "$(jget "$out" .in_offpeak)" "true"
assert_eq "$(jget "$out" .minutes_to_offpeak)" "0"
assert_eq "$(jget "$out" .local_hhmm)" "02:00"

test_case "14:00 is peak; off-peak begins in 480 min (8h to 22:00)"
out="$(bash "$OW" --now 50400 --tz-offset-min 0)"
assert_eq "$(jget "$out" .in_offpeak)" "false"
assert_eq "$(jget "$out" .minutes_to_offpeak)" "480"

test_case "22:00 boundary is inside (start inclusive)"
assert_eq "$(jget "$(bash "$OW" --now 79200 --tz-offset-min 0)" .in_offpeak)" "true"

test_case "08:00 boundary is outside (end exclusive); off-peak in 840 min"
out="$(bash "$OW" --now 28800 --tz-offset-min 0)"
assert_eq "$(jget "$out" .in_offpeak)" "false"
assert_eq "$(jget "$out" .minutes_to_offpeak)" "840"

test_case "07:59 still inside"
assert_eq "$(jget "$(bash "$OW" --now 28740 --tz-offset-min 0)" .in_offpeak)" "true"

test_case "reset at 03:00 lands inside the window, 60 min away"
out="$(bash "$OW" --now 7200 --reset 10800 --tz-offset-min 0)"
assert_eq "$(jget "$out" .minutes_to_reset)" "60"
assert_eq "$(jget "$out" .reset_in_window)" "true"

test_case "reset at 14:00 lands in peak (outside window)"
assert_eq "$(jget "$(bash "$OW" --now 7200 --reset 50400 --tz-offset-min 0)" .reset_in_window)" "false"

test_case "no --reset → reset fields are null"
out="$(bash "$OW" --now 7200 --tz-offset-min 0)"
assert_eq "$(jget "$out" .minutes_to_reset)" "null"
assert_eq "$(jget "$out" .reset_in_window)" "null"

test_case "tz offset shifts the clock: UTC-7 makes epoch-02:00 actually 19:00 (peak)"
assert_eq "$(jget "$(bash "$OW" --now 7200 --tz-offset-min -420)" .in_offpeak)" "false"

test_case "non-wrapping window 01:00–05:00: 02:00 inside, 06:00 outside"
assert_eq "$(jget "$(bash "$OW" --now 7200  --start 01:00 --end 05:00 --tz-offset-min 0)" .in_offpeak)" "true"
assert_eq "$(jget "$(bash "$OW" --now 21600 --start 01:00 --end 05:00 --tz-offset-min 0)" .in_offpeak)" "false"

test_case "missing --now is an error (exit 2)"
assert_exit 2 bash "$OW" --tz-offset-min 0

# ---------- offpeak-budget.sh ----------
R=1000000   # an arbitrary current-window reset epoch; W = 5h = 18000s

test_case "login 9h out → 2 unattended windows (85%) + 1 login window (40%)"
out="$(bash "$OB" --now $((R-3600)) --login $((R+32400)) --reset $R)"
assert_eq "$(jget "$out" .login_window_index)" "2"
assert_eq "$(jget "$out" .unattended_windows)" "2"
assert_eq "$(jget "$out" .current_headroom)" "15"
assert_eq "$(jget "$out" '.windows | length')" "3"
assert_eq "$(jget "$out" '.windows[0].role')" "unattended"
assert_eq "$(jget "$out" '.windows[0].ceiling_pct')" "85"
assert_eq "$(jget "$out" '.windows[2].role')" "login"
assert_eq "$(jget "$out" '.windows[2].ceiling_pct')" "40"
assert_eq "$(jget "$out" '.windows[2].headroom')" "60"

test_case "window ends_at math: window 2 ends 10h after reset"
out="$(bash "$OB" --now $((R-3600)) --login $((R+32400)) --reset $R)"
assert_eq "$(jget "$out" '.windows[2].ends_at')" "$((R+36000))"

test_case "login before reset → current window IS the handover (index 0, reserve applies now)"
out="$(bash "$OB" --now $((R-3600)) --login $((R-100)) --reset $R)"
assert_eq "$(jget "$out" .login_window_index)" "0"
assert_eq "$(jget "$out" .unattended_windows)" "0"
assert_eq "$(jget "$out" .current_headroom)" "60"
assert_eq "$(jget "$out" '.windows | length')" "1"
assert_eq "$(jget "$out" '.windows[0].role')" "login"

test_case "early/peak login → bigger morning reserve (70) lowers login ceiling to 30"
out="$(bash "$OB" --now $((R-3600)) --login $((R+100)) --reset $R --morning-reserve 70)"
assert_eq "$(jget "$out" '.windows[1].ceiling_pct')" "30"

test_case "custom headroom 10 → unattended ceiling 90"
out="$(bash "$OB" --now $((R-3600)) --login $((R+32400)) --reset $R --headroom 10)"
assert_eq "$(jget "$out" '.windows[0].ceiling_pct')" "90"

test_case "login exactly at a window boundary stays in that window (ceil)"
# login = R + 18000 (exactly window-1 end) → still window 1
out="$(bash "$OB" --now $((R-3600)) --login $((R+18000)) --reset $R)"
assert_eq "$(jget "$out" .login_window_index)" "1"

test_case "missing --login is an error"
assert_exit 2 bash "$OB" --now $((R-3600)) --reset $R

finish
