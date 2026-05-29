#!/usr/bin/env bash
# Verify the `system_power` provider's capability snapshot on a real host.
#
# Does NOT actually shut down, restart, or suspend the machine — only
# queries the provider's capability flags and prints them.
#
# Run from the repo root. Requires nix-shell and a host with logind.
set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_DIR"

echo "[1/3] Build quantumd + quantumctl (host nix-shell)..."
nix-shell --run "cargo build --bin quantumd --bin quantumctl"

echo "[2/3] Restart daemon..."
pkill -TERM quantumd 2>/dev/null || true
sleep 1
RUST_LOG=info nix-shell --run "./target/debug/quantumd" &
DAEMON_PID=$!
sleep 4

echo "[3/3] Query system_power capabilities..."
nix-shell --run "./target/debug/quantumctl call provider.query --id system_power" 2>&1 \
    | head -20 \
    || echo "  (query failed)"

echo
echo "Capabilities above reflect what logind reports for the current"
echo "session and user. Lock capability is true if any of"
echo "hyprlock/swaylock/gtklock/loginctl resolved on PATH, OR the user"
echo "set [system_power] lock_command in their config."
echo
echo "This script does NOT exercise the actual power transitions — that"
echo "would shut down or suspend the test machine. To smoke-test the lock"
echo "action without real-locking the screen, set lock_command to a"
echo "harmless echo in ~/.config/quantum/config.toml:"
echo
echo "    [system_power]"
echo "    lock_command = \"echo locked\""
echo
echo "then click Lock twice in the bar widget's power menu and look for"
echo "the line in the daemon stdout."
echo
echo "Press Ctrl+C to shut down the daemon."

trap "kill -TERM $DAEMON_PID 2>/dev/null; wait $DAEMON_PID 2>/dev/null; exit 0" SIGINT
wait $DAEMON_PID
