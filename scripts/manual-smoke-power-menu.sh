#!/usr/bin/env bash
# Manual smoke test for the centered power-menu panel + scheduled actions.
#
# Builds the daemon binaries on the host (nix-shell), restarts the
# daemon, opens the centered plugin/power-menu/power-menu view, and walks the
# user through each interaction. Does NOT actually shut down /
# suspend / reboot. The lock command can be overridden in
# ~/.config/quantum/config.toml to `lock_command = "echo locked"` so
# the lock flow is testable without locking the screen.
#
# Run from the repo root. Requires nix-shell and a Hyprland session.
set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_DIR"

echo "[1/4] Rebuild power-menu + bar bundles in container..."
./scripts/devsh.sh bash -c \
    "pnpm -C src/ui/plugins/power-menu/views/power-menu build && \
     pnpm -C src/ui/plugins/bar/views/bar build"

echo "[2/4] Rebuild quantumd + quantumctl on host via nix-shell..."
nix-shell --run "cargo build --bin quantumd --bin quantumctl"

echo "[3/4] Restart daemon..."
pkill -TERM quantumd 2>/dev/null || true
sleep 1
RUST_LOG=info QUANTUM_LAYER_SHELL=1 nix-shell --run "./target/debug/quantumd" \
    >/tmp/quantumd.log 2>&1 &
DAEMON_PID=$!
sleep 4

echo "[4/4] Open the power menu and prompt..."
nix-shell --run "./target/debug/quantumctl call view.show '{\"name\":\"plugin/power-menu/power-menu\"}'"
sleep 2

cat <<'EOF'

================================================================
Power-menu manual smoke checklist
================================================================

A centered modal should now be on screen with the Power title and
one row per available action (Shutdown / Restart / Suspend /
Hibernate / Lock).

Verify each interaction:

1.  HIDE
    - Click outside the card (on the dim backdrop). Menu closes.
      Re-open via the bar's power icon or:
        quantumctl call view.show '{"name":"plugin/power-menu/power-menu"}'
    - Press Escape. Menu closes.
    - Click the close button (top-right \u00d7). Menu closes.

2.  TWO-CLICK CONFIRM
    - Click any row. Label changes to "Confirm <Action>" with a
      red highlight.
    - Wait 3 seconds without clicking. Label reverts.
    - Click again. Action fires. (For safety prefer Lock here \u2014
      see the lock_command tip below.)

3.  SCHEDULE
    - Click "Schedule\u2026" next to a row. Pills appear (Now, 5m,
      15m, 30m, 1h, Custom\u2026).
    - Click "5m". A scheduled job appears at the bottom of the card.
    - Click the X next to the scheduled job. It disappears.

4.  CUSTOM SCHEDULE
    - Click "Schedule\u2026", then "Custom\u2026". A number input + "min"
      + Schedule button appear.
    - Type 2000. Click Schedule. The clamp should treat 2000 as
      1440 (24h cap). Confirm the scheduled job displays "in 24h"
      (approximately).
    - Cancel the job.

5.  LOCK SAFELY
    To test the Lock action without locking the screen, edit
    ~/.config/quantum/config.toml to add:

        [system_power]
        lock_command = "echo locked"

    Restart the daemon. Then click Lock twice. The daemon log
    (tail /tmp/quantumd.log) should show "locked" printed and the
    menu should close without locking the screen.

When done, press Ctrl+C to shut down the daemon.
================================================================

EOF

trap "kill -TERM $DAEMON_PID 2>/dev/null; wait $DAEMON_PID 2>/dev/null; exit 0" SIGINT
wait $DAEMON_PID
