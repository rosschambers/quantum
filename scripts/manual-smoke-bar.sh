#!/usr/bin/env bash
set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_DIR"

echo "[1/4] Rebuild bar bundle (in container)..."
./scripts/devsh.sh bash -c "pnpm -C src/ui install && cd src/ui/themes/default/views/widgets/bar && pnpm build"

echo "[2/4] Rebuild quantumd + quantumctl (on host via nix-shell)..."
nix-shell --run "cargo build --bin quantumd --bin quantumctl"

echo "[3/4] Restart daemon..."
pkill -TERM quantumd 2>/dev/null || true
sleep 1
RUST_LOG=info QUANTUM_LAYER_SHELL=1 nix-shell --run "./target/debug/quantumd" &
DAEMON_PID=$!
sleep 4

echo "[4/4] Show bar widget..."
nix-shell --run "./target/debug/quantumctl show widgets/bar"
sleep 2

echo
echo "Quantum layer-shell surfaces currently visible:"
hyprctl layers 2>/dev/null | grep -E 'namespace.*quantum' || echo '  (none found via hyprctl; the bar should still be visible)'

echo
echo "Bar should now be visible at the top of your screen with CPU%, MEM%, and media controls."
echo "Press Ctrl+C to shut down the daemon."

trap "kill -TERM $DAEMON_PID 2>/dev/null; wait $DAEMON_PID 2>/dev/null; exit 0" SIGINT
wait $DAEMON_PID
