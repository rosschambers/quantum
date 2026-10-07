# Developing Quantum without rebuilding your system

On NixOS, Quantum is normally installed as a Home-Manager package and run as a
systemd user service (`quantum.service`). Rebuilding the whole system to test a
one-line change is slow. This guide shows how to run a **dev daemon** from the
working tree and iterate on the daemon, plugin views, theme widgets, and pure
UI — without `nixos-rebuild`.

> Everything below runs through `./scripts/devsh.sh`, the nix-shell wrapper that
> provides the GTK4 / WebKitGTK / gtk4-layer-shell toolchain and runtime
> libraries. A bare `cargo run` or running `./target/debug/quantumd` directly
> will fail to find those libraries at runtime.

## How views are served (why some loops are faster than others)

A view's name decides where its bytes come from:

- **Plugin views** — `plugin/<plugin>/<view>` (bar, timers, notification-center,
  launcher, timer-create, power-menu). These are **embedded into `quantumd` at
  compile time**: `build.rs` stages each view's built `dist/` and `main.rs`
  compiles it in with `include_dir!`. Changing one needs the frontend rebuilt
  **and** `quantumd` relinked to re-embed it.
- **Theme widgets** — everything else (currently the clock), under
  `src/ui/themes/<theme>/views/...`. These are **read from disk at request
  time** by the theme store, with a disk copy under
  `$XDG_CONFIG_HOME/quantum/themes/<theme>/` shadowing the embedded copy
  per file. No `quantumd` rebuild needed — just rebuild the view and reload.

There is currently **no IPC method that reloads a view's page**. `view.show` /
`view.hide` / `view.toggle` only toggle visibility on an already-constructed
window; they do not re-fetch the URL. To pick up new view bytes you either
**restart the dev daemon** or, with the WebKit inspector enabled
(`QUANTUM_INSPECTOR=1`), right-click a view → Reload.

## The dev daemon

### 1. Stop the installed service (frees the socket)

`quantumd` binds a single-instance socket at `$XDG_RUNTIME_DIR/quantum.sock`. If
the installed service holds it, a second instance prints `quantum is already
running` and exits — and the service's `Restart=on-failure` / `RestartSec=2`
will respawn it if you just `kill` it. So stop the unit first:

```bash
systemctl --user stop quantum.service
```

Restore it when you are done:

```bash
systemctl --user start quantum.service
```

### 2. Build and run the dev daemon

```bash
# Build once (cold build is a few minutes; warm relink is fast).
./scripts/devsh.sh cargo build --bin quantumd

# Run it detached as a transient unit, with logging and the WebKit inspector.
systemd-run --user --unit=quantum-dev --working-directory="$PWD" \
  --setenv=RUST_LOG=info \
  --setenv=QUANTUM_LAYER_SHELL=1 \
  --setenv=QUANTUM_INSPECTOR=1 \
  bash -c './scripts/devsh.sh ./target/debug/quantumd > /tmp/quantum-dev.log 2>&1'

# Watch logs:
tail -f /tmp/quantum-dev.log

# Tear down the dev daemon:
systemctl --user stop quantum-dev
```

Notes:
- `QUANTUM_LAYER_SHELL=1` matches the installed service so panels anchor like
  production. The bar and overlays always use layer-shell regardless.
- `QUANTUM_INSPECTOR=1` enables WebKit developer extras (right-click → Inspect /
  Reload) and pipes view `console.*` to the daemon's stdout (your log file).
- Running the prebuilt binary through `devsh.sh` supplies the runtime libraries
  without a recompile. Do **not** use `just dev` / `cargo run` for quick
  relaunches — a fresh `cargo run` can trigger a full `-sys` recompile.

### Coexisting with the system bar (advanced)

To run a dev daemon **alongside** the installed one, give it a separate socket
so the single-instance guard doesn't fire:

```bash
./scripts/devsh.sh ./target/debug/quantumd --headless --socket=/tmp/quantum-dev.sock
# talk to it explicitly:
./scripts/devsh.sh ./target/debug/quantumctl --socket=/tmp/quantum-dev.sock <cmd>
```

`--headless` runs the daemon without the GTK UI (no windows), which is ideal for
testing providers, IPC, and search/launch logic without a second bar stacking on
your screen. For full GUI testing, prefer stopping `quantum.service` (above) so
there is only one bar.

## Dev loops by what you changed

### (a) Rust / daemon changes

```bash
./scripts/devsh.sh cargo build --bin quantumd
systemctl --user restart quantum-dev      # relaunch the dev daemon
```

The relink is far faster than a nix system rebuild. Keep `quantum.service`
stopped while iterating.

### (b) Plugin-view (Svelte) changes — bar, timers, launcher, notification-center, power-menu

Set `QUANTUM_PLUGIN_DIR` to the plugins root and the daemon serves each view's
`dist/` straight from the working tree — **no `quantumd` recompile**. Launch the
dev daemon once with the env var (the `systemd-run` block above; or run it in
the foreground):

```bash
QUANTUM_PLUGIN_DIR=src/ui/plugins RUST_LOG=info QUANTUM_INSPECTOR=1 \
  ./scripts/devsh.sh ./target/debug/quantumd
# startup log: "dev plugin mode: serving plugin views from src/ui/plugins"
```

Then the loop is just:

```bash
# 1. Rebuild the view you changed:
./scripts/devsh.sh pnpm -C src/ui/plugins/bar/views/bar build
# 2. Reload it: right-click the view -> Reload (QUANTUM_INSPECTOR=1),
#    or restart the dev daemon to re-read all views from disk:
systemctl --user restart quantum-dev
```

`QUANTUM_PLUGIN_DIR` is additive and opt-in: unset, the daemon uses the
compiled-in embedded views exactly as in production. When set, dev-dir views
take precedence over embedded ones (and a user plugin under
`$XDG_CONFIG_HOME/quantum/plugins` still wins over both). The daemon serves the
view's `dist/` from disk on every request, so a rebuilt bundle is picked up on
the next page load — no `build.rs` re-embed, no cargo relink.

> Still not fully live: there is no `view.reload` IPC yet, so picking up new
> bytes needs an inspector reload or a daemon restart. A `view.reload` method
> plus a plugin-dir watcher would close that gap (see the note at the end).

### (c) Theme-widget changes — the clock

Theme views serve live from disk, so no cargo at all. Shadow the embedded theme
with a disk copy under `$XDG_CONFIG_HOME/quantum/themes/`:

```bash
./scripts/devsh.sh pnpm -C src/ui/themes/default/views/widgets/clock build
mkdir -p ~/.config/quantum/themes/default/views/widgets/clock
cp -r src/ui/themes/default/views/widgets/clock/dist/* \
      ~/.config/quantum/themes/default/views/widgets/clock/
# reload the page: WebKit inspector → Reload, or restart the dev daemon
```

For **token / color** edits (`tokens.toml`), the live recolor path already
exists — run the theme watcher and edit tokens:

```bash
./scripts/devsh.sh cargo run -p quantum-dev -- watch
# edit src/ui/themes/<theme>/tokens.toml -> the live page recolors via theme.reload
```

`quantum-dev watch` only watches `src/ui/themes` and only pushes new token CSS
into the live page; it does not reload view HTML/JS or touch plugin views.

### (d) Pure UI iteration in a browser (fastest, no daemon)

Each view is a standalone Vite app with HMR:

```bash
./scripts/devsh.sh pnpm -C src/ui/plugins/bar/views/bar dev
# open the printed http://localhost:5173 in a browser
```

The IPC bridge (`window.__quantum_*`, `window.quantum.createClient`) only exists
inside the WebKit host, so in a plain browser you must mock it. Add a dev-only
shim (imported from the view's entry, guarded by `import.meta.env.DEV`) that
fakes:

- `window.webkit.messageHandlers.quantum.postMessage` — capture outgoing
  JSON-RPC; optionally answer via `window.__quantum_resolve(id, result)`.
- `window.__quantum_resolve(id, result)` / `window.__quantum_reject(id, err)`.
- `window.__quantum_notify(channel, payload)` — push fake provider events to
  drive `@quantum/client` subscriptions.
- `window.quantum.createClient` — for any direct consumers.

This is ideal for layout, styling, and component logic with instant HMR; it
cannot exercise real providers. (The interactive HTML playgrounds under
`/tmp/opencode/` were built this way.)

## Viewer and explorer verification

The standalone image transport is viewer-owned: `quantum://viewer-image/<token>`
serves an already-open regular file only to its owning WebView. Grants are revoked
on a new read, navigation, hide, or destruction; stale asynchronous responses cannot
restore them. Tokens are not filesystem paths, and an image extension alone grants
no access. Do not replace this with an arbitrary file-read endpoint or disable the
WebKit sandbox. Explorer thumbnails and Markdown image embedding are separate paths.

Run the ordinary checks from the repository root, with dependencies installed:

```bash
./scripts/devsh.sh bash -c 'pnpm -C src/ui -r build && cargo build --locked -p quantumd && cargo test --locked --workspace && cargo fmt --all -- --check'
./scripts/devsh.sh pnpm -C src/ui -r test
./scripts/devsh.sh cargo clippy --locked --workspace --all-targets -- -D warnings
git diff --check
```

### Isolated native viewer test

`src/ui/host/tests/native_file_image.rs` is feature-gated and ignored by default.
It tests the real WebKit host bridge and image grants; setting the bundle directory
also tests the built viewer application, controls, search, and layout against a
fixture dispatcher, not the installed daemon or integrated filesystem backend.
Never run it on the desktop or stop the installed service for this check.

Require `xvfb-run`, `dbus-run-session`, `jq`, and `timeout` on `PATH` inside the
development shell. If missing on NixOS, enter `nix-shell -p xvfb-run dbus jq`
first; this does not activate system configuration. After the frontend build above,
run from the repository root (the test currently also requires `/tmp/opencode`):

```bash
./scripts/devsh.sh bash <<'BASH'
set -euo pipefail
command -v xvfb-run dbus-run-session jq timeout
TEST_BINARY="$(cargo test --locked -p quantum-ui --features gtk-test \
  --test native_file_image --no-run --message-format=json | \
  jq -r 'select(.reason == "compiler-artifact" and .target.name == "native_file_image" and .profile.test == true and .executable != null) | .executable')"
test -x "$TEST_BINARY"
mkdir -p /tmp/opencode
SESSION_ROOT="$(mktemp -d /tmp/opencode/quantum-native.XXXXXX)"
trap 'rm -rf -- "$SESSION_ROOT"' EXIT
mkdir -p "$SESSION_ROOT"/{home,runtime,config,cache,state,data}
chmod 700 "$SESSION_ROOT/runtime"
ulimit -c 0
env -i PATH="$PATH" LD_LIBRARY_PATH="${LD_LIBRARY_PATH:-}" \
  HOME="$SESSION_ROOT/home" XDG_RUNTIME_DIR="$SESSION_ROOT/runtime" \
  XDG_CONFIG_HOME="$SESSION_ROOT/config" XDG_CACHE_HOME="$SESSION_ROOT/cache" \
  XDG_STATE_HOME="$SESSION_ROOT/state" XDG_DATA_HOME="$SESSION_ROOT/data" \
  XDG_DATA_DIRS="${XDG_DATA_DIRS:-/run/current-system/sw/share}" \
  GDK_BACKEND=x11 GTK_A11Y=none QUANTUM_NATIVE_TEST_ISOLATED=1 \
  QUANTUM_NATIVE_VIEWER_DIST="$PWD/src/ui/plugins/file-viewer/views/file-viewer/dist" \
  QUANTUM_NATIVE_IMAGE_FIXTURES="${QUANTUM_NATIVE_IMAGE_FIXTURES:-}" \
  xvfb-run --auto-servernum --server-args='-screen 0 1024x768x24 -nolisten tcp' \
  dbus-run-session -- timeout 100 "$TEST_BINARY" --ignored --nocapture --test-threads=1
BASH
```

The clean environment excludes the desktop display, Wayland sockets, session bus,
and sandbox overrides; Xvfb and DBus create private sessions. No hashed executable
name, Nix store path, or temporary helper script is required.

SVG and PNG fixtures are internal. For the other six formats, export
`QUANTUM_NATIVE_IMAGE_FIXTURES` as an absolute directory containing real 16-by-16
pixel files named `fixture.jpg`, `fixture.gif`, `fixture.webp`, `fixture.bmp`,
`fixture.ico`, and `fixture.avif` before running the block. Supply these externally;
the repository does not contain the temporary fixture generator used during development.
Missing fixtures log `UNAVAILABLE`; optional formats may report a visible decode
error without failing the test. An eight-format claim therefore requires all six
fixtures and checking each logged `built viewer format=` observation for
`"loaded":true`, not just the final pass count. The test logs bundle hashes.

Native built-bundle coverage includes virtualized text search, nested JSON fold
reveal, rendered Markdown search excluding Mermaid, image controls and corrupt-image
errors, special-character filenames, foreign/revoked grants, and asynchronous
hide/navigation/read races. Component tests also cover code search and drag-to-pan.
Desktop interaction feel and installed-service verification remain separate checks;
ask before opening desktop windows, and never run repeated desktop test loops.

## Quick reference

| You changed                  | Command(s)                                                                 | Reload |
| ---------------------------- | ------------------------------------------------------------------------- | ------ |
| Rust / daemon                | `cargo build --bin quantumd` → `systemctl --user restart quantum-dev`     | restart |
| Plugin view (`QUANTUM_PLUGIN_DIR=src/ui/plugins`) | `pnpm -C <view> build`                               | inspector reload / restart (no cargo) |
| Theme widget (clock)         | `pnpm -C <view> build` → copy `dist/` to `~/.config/quantum/themes/...`    | inspector reload / restart |
| Theme tokens (colors)        | `quantum-dev watch` + edit `tokens.toml`                                   | live (recolor only) |
| Pure UI / layout             | `pnpm -C <view> dev` (browser + mock bridge)                              | live (HMR) |

All `cargo` / `pnpm` commands are prefixed with `./scripts/devsh.sh`.

## Fully-live plugin reload (future)

`QUANTUM_PLUGIN_DIR` (loop (b)) removes the cargo recompile, but picking up new
bytes still needs an inspector reload or a daemon restart, because there is no
IPC that reloads a view's page (`view.show/hide/toggle` only toggle visibility;
only an internal `WindowRequest::Close` reconstructs a window, and it is not
exposed over IPC). Closing that gap would take two additions:

- a `view.reload` (or `view.close`) IPC method that drops the window so the next
  open re-fetches the URL, and
- a plugin-dir file watcher (extend `quantum-dev watch`, which today only
  watches `src/ui/themes` and only triggers a token recolor) that fires
  `view.reload` when a plugin `dist/` changes.

With those, `pnpm -C <view> build` alone would live-reload the running view.
