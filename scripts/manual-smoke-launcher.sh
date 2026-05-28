#!/usr/bin/env bash
set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_DIR"

echo "[1/4] Rebuild Svelte launcher bundle (in container)..."
./scripts/devsh.sh bash -c "pnpm -C frontend install && cd frontend/themes/default/views/launcher && pnpm build"

echo "[2/4] Rebuild quantumd + quantumctl (on host via nix-shell)..."
nix-shell --run "cargo build --bin quantumd --bin quantumctl"

echo "[3/4] Restart daemon..."
pkill -TERM quantumd 2>/dev/null || true
sleep 1
RUST_LOG=info nix-shell --run "./target/debug/quantumd" &
DAEMON_PID=$!
sleep 2

echo "[4/4] Show launcher..."
nix-shell --run "./target/debug/quantumctl view.show launcher"
sleep 1

echo
echo "Quantum windows currently open:"
hyprctl clients -j | python3 -c "import json,sys; ws=json.load(sys.stdin); [print(f'  {w[\"class\"]:30} {w[\"title\"][:60]}') for w in ws if 'quantum' in (w.get('class','') + w.get('initialClass','')).lower()]"

echo
echo "Launcher should now be visible. Test:"
echo "  - Type 'fire' — Firefox should appear in results"
echo "  - Press Enter — Firefox should launch"
echo "  - Press Esc — launcher hides"
echo
echo "Press Ctrl+C to shut down the daemon."

trap "kill -TERM $DAEMON_PID 2>/dev/null; wait $DAEMON_PID 2>/dev/null; exit 0" SIGINT
wait $DAEMON_PID
