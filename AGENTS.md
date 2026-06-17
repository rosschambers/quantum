# AGENTS.md — Quantum

Working agreement for any agent or human contributor in this repo.

## Project

Quantum is a Wayland/Hyprland launcher and widget host. Single Rust daemon
owns a Tokio runtime and GTK4 main loop, renders Svelte 5 frontends inside
WebKitGTK windows anchored via gtk4-layer-shell. See
`docs/plans/2026-05-26-quantum-launcher-design.md` for the design.

## Architecture — Onion Layers

Crates and their allowed dependencies:

| Crate                          | May depend on                              |
| ------------------------------ | ------------------------------------------ |
| `src/domain`                   | nothing (only `thiserror`, `serde`, `serde_json`, `async-trait`)        |
| `src/application`              | `domain`                                   |
| `src/infrastructure`           | `domain`                                   |
| `src/ui/host`                  | `application` (and through it, `domain`)   |
| `src/binaries/quantumd`        | `ui`, `application`, `infrastructure`      |
| `src/binaries/quantumctl`      | `domain`, `infrastructure` (ipc client)    |
| `src/binaries/quantum-dev`     | `domain`, `infrastructure`                 |

**Forbidden:**
- `domain` importing any other workspace crate or any async/IO crate.
- `application` importing `infrastructure` or `ui`.
- `infrastructure` importing `application` or `ui`.
- `ui` importing `infrastructure` directly (must go through `application`).

A CI test in `tests/architecture-test/src/lib.rs` enforces these rules by parsing
Cargo metadata. Do not weaken it.

**Cross-layer rules in practice:**
- `domain` must not gain non-serde dependencies. Time and calendar logic stays
  pure integer arithmetic (for example weekday plus seconds-into-day) behind a
  domain `Clock` port; `chrono` lives only in `infrastructure`. Do not add
  `chrono` or `time` to `domain`.
- When `application` and `infrastructure` must communicate (for example a
  use-case pushing state to a streaming provider), define a domain port and
  have `infrastructure` implement it, then wire the two together in `quantumd`.
  The `TimerBroadcast` port (`TimerService` to `TimerProvider`) is the reference
  pattern — `application` never imports `infrastructure`.

## Commit Style

Conventional commits:
- `feat:` new capability
- `fix:` bug fix
- `chore:` build, deps, tooling
- `docs:` documentation only
- `test:` test changes only
- `refactor:` no behaviour change

Commit per task in the implementation plan. Small commits beat big ones.

## Commands

| Command         | Purpose                                  |
| --------------- | ---------------------------------------- |
| `just build`    | Build all crates                         |
| `just test`     | Run all tests                            |
| `just fmt`      | Format Rust                              |
| `just lint`     | Clippy, deny warnings                    |
| `just dev`      | Run quantumd in dev mode                 |
| `just frontend-install` | Install frontend pnpm dependencies |
| `just frontend-build` | Build all Svelte views (`dist/`)   |
| `just frontend-test` | Run the frontend vitest suites      |

> **There is no `ts-bindgen` / TypeScript codegen.** Despite older mentions,
> `src/ui/packages/client/src/generated.ts` is an empty stub. TypeScript DTOs
> are hand-written in `@quantum/client` (`src/ui/packages/client/src/`)
> mirroring the Rust types — see `timer.ts` and `notifications.ts`. When you add
> or change a Rust DTO that crosses IPC, hand-update the matching TypeScript
> type and re-export it from `index.ts`.

## Build Environment

- **All builds, tests, lint, and format checks run via `./scripts/devsh.sh
  <command>`, which is a thin nix-shell wrapper around `shell.nix`.** The
  host's nixpkgs provides Rust 1.85, GTK4, WebKitGTK 6, and gtk4-layer-shell.
- The audio provider currently shells out to `pactl`, so that binary must be
  available in the host environment.
- **Each `devsh.sh` invocation can trigger a full cargo rebuild.** Build-script
  fingerprints for the GTK/WebKit `-sys` crates depend on the nix-shell
  environment, so a separate `devsh.sh cargo run`/`cargo build` may recompile
  the whole dependency tree from scratch. A clean build is roughly two to three
  minutes even when warm — set generous tool timeouts (ten minutes or more) for
  any build, test, or lint command. To run the daemon without paying the
  rebuild, build once with `devsh.sh cargo build --bin quantumd`, then launch
  the prebuilt binary directly through the same wrapper:
  `devsh.sh ./target/debug/quantumd` (the wrapper still supplies the GTK and
  WebKit runtime libraries; `cargo run` does not).

## Running and Testing the Daemon Locally

- **Single-instance guard.** `quantumd` binds a Unix socket at
  `$XDG_RUNTIME_DIR/quantum.sock` (`src/binaries/quantumd/src/main.rs:782`).
  If a live daemon already owns it, a new instance prints `quantum is already
  running` and exits with code 1. A stale socket is removed automatically only
  when connecting to it fails, so a crashed daemon's socket self-heals but a
  live one blocks you.
- **A systemd user service respawns the daemon.** `quantum.service` (enabled,
  running the nix-installed binary — NOT your local dev build) restarts
  `quantumd` after you `kill` the process, re-owning the socket within
  seconds. To run a dev build you must `systemctl --user stop quantum.service`
  first; killing the process alone is not enough. Restore normal operation with
  `systemctl --user start quantum.service`. The installed service does not pick
  up local code changes until the nix package is rebuilt and reinstalled, so a
  fix verified against a dev build is not live on the user's session until then.
- **Detaching a dev daemon.** Launch with `setsid` and redirect to a log file
  so it survives the spawning shell, and run with `RUST_LOG=info` to capture
  provider registration, `ViewMultiplexer` install, and the per-monitor
  `opening/closing per-monitor view window` lines used to verify monitor
  hotplug.
- **Cleanest detached launch: `systemd-run --user`.** `systemd-run --user
  --unit=quantum-dev --working-directory="$PWD" --setenv=RUST_LOG=info bash -c
  './scripts/devsh.sh ./target/debug/quantumd > /tmp/quantum-dev.log 2>&1'`
  returns immediately and runs the daemon as a transient user unit; plain `&`
  backgrounding can leave the spawning shell hanging on the inherited pipe. Tear
  down with `systemctl --user stop quantum-dev`, then `systemctl --user start
  quantum.service` to restore the installed daemon.

## Provider and Event Conventions

- `ProviderSource` trait (in `src/domain/src/ports.rs`) has an optional
  `fn subscribe(&self) -> Option<BoxStream<'static, serde_json::Value>>`.
  Streaming providers override it; one-shot query providers do not.
- `EventBus::publish` takes `(channel: &str, payload: &str)`, not an
  `EventEnvelope`. The daemon's `BroadcastingEventBus` adapter converts
  string payloads to `EventEnvelope` for the broadcast channel.
- **Channel naming**: `SubscribeProviderUseCase` publishes on
  `format!("{provider_id}.event")`. Provider id `mpris` publishes on
  `mpris.event`. Frontend subscriptions must match exactly.
- `Action::Custom { kind, payload }`: outer dispatcher envelope is
  `{"kind": "custom", "data": {"kind": "<provider>", "payload": {...}}}`.
  The inner `payload` (not `data`) carries provider-specific command fields.
- Register streaming providers in `src/binaries/quantumd/src/main.rs` AND
  pre-subscribe them at startup via `SubscribeProviderUseCase` so events
  start flowing before any frontend connects.
- **Timers subsystem.** Provider id `timer` publishes on `timer.event` with an
  enriched envelope `{ change, settings, timers }`. Dispatcher IPC methods are
  `timer.create` / `timer.list` / `timer.edit` / `timer.cancel` /
  `timer.dismiss` / `timer.dismiss_all`. Timer state persists to a writable JSON
  store at `$XDG_STATE_HOME/quantum/timers.json` (atomic temp-file plus rename)
  — the first and only writable store in the project; `ConfigStore` remains
  read-only TOML. Re-armed on startup via `TimerService::load_and_arm`.

## UI and Frontend Conventions

- **Layer-shell usage differs by window type — `QUANTUM_LAYER_SHELL` gates
  only plain panels, not the bar.** `WidgetWindow` (the bar and the clock)
  always calls `init_layer_shell()` unconditionally
  (`src/ui/host/src/windows/widget.rs:94` and `:198`). `PanelWindow` decides
  via `should_use_layer_shell(env_flag, is_overlay) = env_flag || is_overlay`
  (`src/ui/host/src/windows/panel.rs:61`): overlays (power-menu and similar)
  always use layer-shell because they dim the whole screen and dismiss on
  Escape, so they cannot lock the user out; plain panels (the launcher) use it
  only when `QUANTUM_LAYER_SHELL=1`, defaulting otherwise to a normal
  xdg-toplevel so a stuck launcher window cannot lock the user out.
- **Per-monitor windows and the GDK Wayland connector race.** The bar is a
  per-monitor widget spawned by `ViewMultiplexer`
  (`src/ui/host/src/multiplex.rs`): one window per connected monitor, keyed
  `plugin/bar/bar@<connector>`. On Wayland a hot-plugged `gdk::Monitor` can
  enter `gdk::Display::monitors()` (firing `items-changed`) *before* its
  `connector` name arrives in a later roundtrip, and GDK does not re-fire
  `items-changed` when the name later populates — that is a per-object
  `notify::connector`. Any code reacting to monitor changes through
  `items-changed` alone will silently miss connector-less monitors (the bar
  never appears on the new output until another monitor event fires). The
  multiplexer handles this by subscribing to `notify::connector` on
  connector-less monitors (`wire_connector_arrival`) and re-running its sync
  when the name arrives; `diff_emit` is idempotent so redundant fires are
  harmless. Preserve this pattern when touching monitor enumeration.
  Verify hotplug with `RUST_LOG=info`: look for the `deferring bar spawn until
  notify::connector` warning followed by `opening per-monitor view window:
  plugin/bar/bar@<connector>` for the new output.
- All Vite views must set `base: './'` in `vite.config.ts` — the
  `quantum://` custom URI scheme breaks absolute URL normalization.
- Widget URLs are `quantum://theme/<theme>/views/widgets/<name>/index.html`.
  The `views/` prefix is required and is handled by `candidate_paths` in
  `src/infrastructure/src/theme/store.rs`.
- Always wrap window notify calls in
  `if (typeof window.__quantum_notify === 'function') { ... }` — events can
  arrive before the JS client has loaded.
- Svelte 5 components used in vitest must use `$effect` for setup work, not
  `onMount`. testing-library's legacy adapter does not fire `onMount`
  reliably under Svelte 5 runes mode.
- All Svelte views consume IPC through `@quantum/client`. Never reach into
  `window.__quantum_*` directly from view code.
- **Background-layer widgets honor `ViewPosition`, and `fill_output` fills the
  monitor.** A non-bar `WidgetWindow` sits on `Layer::Background` and anchors
  per the descriptor's `ViewPosition` (`Center` maps to top-right for backward
  compatibility with the clock). Set `fill_output = true` in `view.toml` to make
  the surface span the whole monitor (anchors all four edges) — required for
  free-placement / scatter widgets such as the timers view; omit it for
  content-sized widgets such as the clock.
- **Layer-shell surfaces capture pointer input across their whole region, even
  when transparent.** A mapped overlay/background surface swallows clicks over
  its area. Until input-region passthrough is implemented (known follow-up),
  transient surfaces (toasts) must be HIDDEN when empty via `view.hide`, and
  full-screen background widgets (timers) will capture bare-desktop clicks in
  their empty regions. Keep this in mind for any new layer-shell window.
- **The `ViewMultiplexer` only auto-opens views with BOTH `per_monitor = true`
  and `auto_show = true`** (`src/binaries/quantumd/src/main.rs`). A
  `per_monitor = false, auto_show = true` view is not opened by the multiplexer;
  open it on demand by canonical name via `view.show` / `view.toggle`.
- **First-party plugin views are embedded from their built `dist/`.**
  `src/binaries/quantumd/build.rs` auto-discovers any
  `src/ui/plugins/<plugin>/views/<view>/` containing a `dist/index.html` and
  compiles it in. Build the view's `dist/` (`pnpm ... build` or
  `just frontend-build`) BEFORE building `quantumd`; a brand-new plugin may need
  a SECOND `cargo build` to be embedded, because its `rerun-if-changed` paths
  are only registered once the build script has first seen the directory.

## Rules

- No `unwrap`/`expect` outside tests and `main` setup.
- Errors are typed per-layer. Never leak Rust types across IPC.
- TDD. Write the failing test first.
- No emojis in source or commits.
- No abbreviations in code, comments, or docs. Standard acronyms (JSON,
  DBus, IPC, GTK) are fine because they are names, not abbreviations.
- Verify before claiming done — run the actual command, read the actual
  output. No "should work" claims.
- Never `git commit --amend` without explicit user permission.
