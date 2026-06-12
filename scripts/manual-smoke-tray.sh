#!/usr/bin/env bash
# Exercise the six tray providers on a real host.
#
# Builds the daemon (in nix-shell so it links against the host Wayland
# stack), starts it, queries each tray provider once, prints its
# `available` field, then opens the bar widget so the tray indicators
# can be inspected visually.
#
# Run from the repo root. Requires nix-shell and a Hyprland session.
set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_DIR"

echo "[1/4] Rebuild bar bundle (in container)..."
./scripts/devsh.sh bash -c "pnpm -C src/ui install && cd src/ui/plugins/bar/views/bar && pnpm build"

echo "[2/4] Rebuild quantumd + quantumctl (on host via nix-shell)..."
nix-shell --run "cargo build --bin quantumd --bin quantumctl"

echo "[3/4] Restart daemon..."
pkill -TERM quantumd 2>/dev/null || true
sleep 1
RUST_LOG=info QUANTUM_LAYER_SHELL=1 nix-shell --run "./target/debug/quantumd" &
DAEMON_PID=$!
sleep 4

echo "[4/4] Query each tray provider..."
for id in power network bluetooth power_profile audio brightness; do
    echo "  --- $id ---"
    nix-shell --run "./target/debug/quantumctl call provider.query --id $id" 2>&1 \
        | head -20 \
        || echo "  (query failed for $id)"
done

echo
echo "Opening bar widget..."
nix-shell --run "./target/debug/quantumctl show widgets/bar"
sleep 2

echo
echo "Bar should be visible with: ActiveWindow | CPU/MEM | Media | Tray"
echo "Tray order: Network, Bluetooth, Volume, Brightness, Battery, PowerProfile."
echo "Indicators whose backend is missing render empty (no slot reserved)."
echo
echo "Press Ctrl+C to shut down the daemon."

trap "kill -TERM $DAEMON_PID 2>/dev/null; wait $DAEMON_PID 2>/dev/null; exit 0" SIGINT
wait $DAEMON_PID
