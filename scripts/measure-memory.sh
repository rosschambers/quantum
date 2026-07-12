#!/usr/bin/env bash
set -euo pipefail

# Report resident (RSS) and proportional (PSS) memory for the running quantumd
# daemon and all of its WebKit render/network processes, plus a total PSS and
# the live WebKitWebProcess count. PSS is the honest cross-process figure
# because it counts shared libraries once; RSS overcounts them.

# Match the daemon by process name. On nix-wrapped installs the comm is
# ".quantumd-wrapped" (truncated to ".quantumd-wrapp"), so an exact "quantumd"
# match misses it; fall back to a substring match. Take the first hit.
DAEMON_PID="$(pgrep -x quantumd 2>/dev/null | head -n1 || true)"
if [ -z "$DAEMON_PID" ]; then
  DAEMON_PID="$(pgrep quantumd 2>/dev/null | head -n1 || true)"
fi
if [ -z "$DAEMON_PID" ]; then
  echo "quantumd not running"
  exit 0
fi

pss_of() { awk '/^Pss:/{print $2}' "/proc/$1/smaps_rollup" 2>/dev/null || echo 0; }
rss_of() { awk '/^Rss:/{print $2}' "/proc/$1/smaps_rollup" 2>/dev/null || echo 0; }

# Collect the daemon plus every WebKit render/network process, deduplicated by
# PID. WebKit processes are matched by process name (comm), which the kernel
# truncates to 15 characters: "WebKitWebProcess" (16) becomes "WebKitWebProces"
# and "WebKitNetworkProcess" becomes "WebKitNetworkPr". Matching comm (not the
# full command line via -f) deliberately excludes the bwrap sandbox launchers,
# whose command line contains the WebKitWebProcess path but whose comm is
# "bwrap"; it also excludes the daemon's non-WebKit children (a launched app
# such as Spotify, pactl) so the total reflects Quantum's own renderers.
#
# Caveat: this matches WebKit processes host-wide. On this system Quantum is the
# only WebKitGTK application; if another WebKitGTK app runs concurrently its
# renderers would be included.
PIDS="$(
  {
    echo "$DAEMON_PID"
    pgrep WebKitWebProces 2>/dev/null || true
    pgrep WebKitNetworkPr 2>/dev/null || true
  } | sort -u -n
)"

TOTAL=0
printf '%-8s %-24s %10s %10s\n' PID COMMAND RSS_KB PSS_KB
for pid in $PIDS; do
  [ -d "/proc/$pid" ] || continue
  comm="$(tr -d '\0' < "/proc/$pid/comm" 2>/dev/null || echo '?')"
  rss="$(rss_of "$pid")"
  pss="$(pss_of "$pid")"
  printf '%-8s %-24s %10s %10s\n' "$pid" "$comm" "$rss" "$pss"
  TOTAL=$((TOTAL + pss))
done
echo "----"
echo "TOTAL PSS (KB): $TOTAL"
echo "WebKitWebProcess count: $(pgrep -c WebKitWebProces || echo 0)"
