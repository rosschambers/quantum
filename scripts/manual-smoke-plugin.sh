#!/usr/bin/env bash
# Manual end-to-end smoke test for the plugin system.
# Installs a moon-distance fake plugin into the user's plugins dir,
# then exercises provider.query, view.show, and action.invoke against
# a running quantumd.
#
# Usage:
#   1. nix-shell --run "cargo build --bin quantumd"
#   2. Start the daemon (see scripts/manual-smoke-bar.sh for the
#      reference startup command).
#   3. ./scripts/manual-smoke-plugin.sh

set -euo pipefail

PLUGIN_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/quantum/plugins/moon-distance"

echo "Installing moon-distance plugin under $PLUGIN_DIR ..."
mkdir -p "$PLUGIN_DIR/scripts" "$PLUGIN_DIR/actions" "$PLUGIN_DIR/views/moon-widget"

cat > "$PLUGIN_DIR/config.toml" <<'INNER'
[scripts.moon-distance]
interval = 5
INNER

cat > "$PLUGIN_DIR/scripts/moon-distance" <<'INNER'
#!/usr/bin/env bash
# Smoke test: print a constant payload so the test is deterministic.
echo '{"distance_km": 384400}'
INNER
chmod +x "$PLUGIN_DIR/scripts/moon-distance"

cat > "$PLUGIN_DIR/actions/open-calendar" <<'INNER'
#!/usr/bin/env bash
# Returns its response on stdout. xdg-open is best-effort; the
# important thing is the IPC routing.
echo '{"opened": true}'
INNER
chmod +x "$PLUGIN_DIR/actions/open-calendar"

cat > "$PLUGIN_DIR/views/moon-widget/index.html" <<'INNER'
<!doctype html>
<html>
<head><meta charset="utf-8" /></head>
<body>
  <button id="moon-btn">Moon: ... km</button>
  <script>
    const client = window.quantum.createClient();
    const btn = document.getElementById('moon-btn');
    function render(state) {
      if (!state) return;
      btn.textContent = 'Moon: ' + Math.round(state.distance_km / 1000) + ' thousand km';
    }
    client.call('provider.query', { id: 'moon-distance.moon-distance' }).then(render);
    client.subscribe('moon-distance.moon-distance', render);
    btn.addEventListener('click', () => {
      client.call('action.invoke', { provider: 'moon-distance', action: 'open-calendar' });
    });
  </script>
</body>
</html>
INNER

echo
echo "Plugin installed. Now (with the daemon running):"
echo
echo "  Check the startup log for 'Registered plugin moon-distance'."
echo "  Then run the following IPC checks:"
echo
echo "  ./target/debug/quantumctl call provider.query '{\"id\":\"moon-distance.moon-distance\"}'"
echo "    -> {\"distance_km\":384400} (after the first 5s tick)"
echo
echo "  ./target/debug/quantumctl call view.show '{\"name\":\"plugin/moon-distance/moon-widget\"}'"
echo "    -> a WebKit window opens with 'Moon: 384 thousand km'"
echo
echo "  ./target/debug/quantumctl call plugin.reload '{}'"
echo "    -> {\"loaded\":1}"
echo
echo "  ./target/debug/quantumctl call action.invoke \\"
echo "      '{\"provider\":\"moon-distance\",\"action\":\"open-calendar\"}'"
echo "    -> {\"opened\":true} returned over IPC"
