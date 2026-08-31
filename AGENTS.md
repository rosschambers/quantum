# AGENTS.md — Quantum

Working agreement for any agent or human contributor in this repo.

## Project

Quantum is a Wayland/Hyprland launcher and widget host. Single Rust daemon
owns a Tokio runtime and GTK4 main loop, renders Svelte 5 frontends inside
WebKitGTK windows anchored via gtk4-layer-shell. See
`docs/architecture.md` for the design.

## Architecture — Onion Layers

Crates and their allowed dependencies:

| Crate                          | May depend on                              |
| ------------------------------ | ------------------------------------------ |
| `src/domain`                   | nothing (only `thiserror`, `serde`, `serde_json`, `async-trait`, `futures`) |
| `src/application`              | `domain`                                   |
| `src/infrastructure/config`    | `domain`, sibling infrastructure crates    |
| `src/infrastructure/dbus`      | `domain`, sibling infrastructure crates    |
| `src/infrastructure/hyprland`  | `domain`, sibling infrastructure crates    |
| `src/infrastructure/ipc`       | `domain`, sibling infrastructure crates    |
| `src/infrastructure/plugins`   | `domain`, sibling infrastructure crates    |
| `src/infrastructure/providers` | `domain`, sibling infrastructure crates    |
| `src/infrastructure/theme-store` | `domain`, sibling infrastructure crates  |
| `src/ui/host`                  | `application` (and through it, `domain`)   |
| `src/binaries/quantumd`        | `ui`, `application`, `infrastructure`      |
| `src/binaries/quantumctl`      | `domain`, `infrastructure` (ipc client)    |
| `src/binaries/quantum-dev`     | `domain`, `infrastructure`                 |

Infrastructure is **seven sibling crates**, not one. Sibling infrastructure
crates may depend on each other (for example `providers` on `dbus` and
`hyprland`); the architecture test in `tests/architecture-test/src/lib.rs`
explicitly permits sibling-on-sibling infrastructure edges.

**Forbidden:**
- `domain` importing any other workspace crate, or any async *runtime* / IO
  crate. `futures` is permitted solely for the `BoxStream` stream type used by
  `ProviderSource::subscribe` in `src/domain/src/ports.rs`; do not add `tokio`,
  `chrono`, `time`, or other runtime/IO crates.
- `application` importing `infrastructure` or `ui`.
- `infrastructure` importing `application` or `ui`.
- `ui` importing `infrastructure` directly (must go through `application`).

A CI test in `tests/architecture-test/src/lib.rs` enforces these rules by parsing
Cargo metadata. Do not weaken it.

**Cross-layer rules in practice:**
- `domain` must not gain new non-serde dependencies beyond the blessed
  `futures` (`BoxStream`) exception noted above. Time and calendar logic stays
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

> **Direct `pnpm` commands run from the pnpm workspace root `src/ui`, NOT the
> repo root.** `pnpm-workspace.yaml` lives at `src/ui/`, so a bare
> `./scripts/devsh.sh pnpm --filter <pkg> ...` from the repo root fails with
> `ERR_PNPM_NO_PKG_MANIFEST` (there is no `package.json` at the repo root). Run
> `./scripts/devsh.sh bash -c "cd src/ui && pnpm --filter <pkg> ..."`, or from a
> view directory directly (`cd src/ui/plugins/<plugin>/views/<view> && pnpm test`
> / `pnpm exec vitest run <pattern>`). The `just frontend-*` recipes run from the
> repo root as normal. Package names for `--filter`: the client is
> `@quantum/client`; the file-explorer view is `default-panel-files` (from its
> `package.json` `name`), not its path. Build `@quantum/client` before any view
> that imports it (topological — never `pnpm -r --parallel`).

## Build Environment

- **All builds, tests, lint, and format checks run via `./scripts/devsh.sh
  <command>`, which is a thin nix-shell wrapper around `shell.nix`.** The
  toolchain comes from the host's nixpkgs (`rustc`/`cargo` from `shell.nix`,
  currently rustc 1.95), along with GTK4, WebKitGTK 6, and gtk4-layer-shell.
  The exact Rust version tracks nixpkgs and can drift. The repo's
  `rust-toolchain.toml` is **not** consulted inside the nix shell — there is no
  rustup there — so it only governs non-nix rustup users; the workspace MSRV
  (`rust-version = "1.85"` in `Cargo.toml`) remains the supported floor.
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
- **The `mold` linker is wired in `shell.nix`'s shellHook, NOT a committed
  `.cargo/config.toml`.** A committed `rustflags = ["-C", "link-arg=-fuse-ld=mold"]`
  applies to *every* cargo build of the repo, including the downstream nix
  package build (`rustPlatform.buildRustPackage`), whose sandbox has no `mold`
  — that breaks `nixos-rebuild` with a linker-not-found error on `quantumctl`.
  Keep mold (and any linker choice) in `shell.nix` so only the dev/CI nix-shell
  uses it and packaged builds fall back to the default linker.
- **Fast local iteration: see `docs/development.md`.** Run a dev daemon instead
  of rebuilding the system, and set `QUANTUM_PLUGIN_DIR=src/ui/plugins` so
  `quantumd` serves plugin views' `dist/` from the working tree (a `pnpm build`
  + reload, no cargo recompile). Theme widgets already serve from disk;
  `quantum-dev watch` hot-reloads theme tokens only.
- **Theme `tokens.toml` is embedded, and the active theme may not be `default`.**
  The `default` and `sycamore` themes are baked into `quantumd` via `include_dir!`
  (`src/infrastructure/theme-store/src/store.rs`), so editing a `tokens.toml`
  only takes effect after a `quantumd` rebuild — and `include_dir` does not
  reliably notice the change, so `touch src/infrastructure/theme-store/src/store.rs`
  to force a recompile. A disk theme at the configured themes directory shadows
  the embedded copy. Check `active_theme` in the config before editing tokens
  (the maintainer's is `sycamore`, not `default`).

## Continuous Integration

`.github/workflows/ci.yml` builds inside the nix-shell (`shell.nix`) on a pinned
`nixos-unstable` revision — NOT via apt (the GTK4/WebKitGTK/gtk4-layer-shell
packages are not apt-installable under those names). Two gotchas that have
broken CI before; do not reintroduce them:
- **The frontend build must stay topological — do NOT use `pnpm -r --parallel
  build`.** The view packages import `@quantum/client`, which must build first;
  a parallel build races and fails to resolve it on a cold tree.
- **The Rust step must `cargo build -p quantumd` before `cargo test
  --workspace`.** The `quantum-e2e` test spawns the compiled
  `target/debug/quantumd`, which `cargo test --no-run` does *not* produce.
- Run `devsh.sh cargo fmt --all` before committing Rust — CI runs
  `cargo fmt --all -- --check` (rustfmt 1.95) and a single unformatted line
  fails the whole run.

## Running and Testing the Daemon Locally

- **Single-instance guard.** `quantumd` binds a Unix socket at
  `$XDG_RUNTIME_DIR/quantum.sock` (the socket-path resolution and
  single-instance guard in `main`, `src/binaries/quantumd/src/main.rs:814-833`).
  If a live daemon already owns it, a new instance prints `quantum is already
  running` and exits with code 1. A stale socket is removed automatically only
  when connecting to it fails, so a crashed daemon's socket self-heals but a
  live one blocks you.
- **A systemd user service respawns the daemon.** `quantum.service` (enabled)
  restarts `quantumd` after you `kill` the process, re-owning the socket within
  seconds, so killing the process alone is never enough to free the socket for a
  dev build. To run a dev build you must `systemctl --user stop quantum.service`
  first, then restore normal operation with `systemctl --user start
  quantum.service`. **Which binary the service runs depends on how it was
  installed**, and the two supported modes differ:
  - **Cargo install (the shipped unit).** `packaging/systemd/quantum.service`
    sets `ExecStart=%h/.cargo/bin/quantumd`, so a service installed from that
    unit runs whatever `cargo install --path src/binaries/quantumd` last placed
    in `~/.cargo/bin`. A dev build is not live until you re-run `cargo install`
    (or point the unit at your `target/debug/quantumd`).
  - **Nix-installed.** If the maintainer's machine instead runs a
    nix-packaged `quantumd`, the service does not pick up local code changes
    until the nix package is rebuilt and reinstalled.
  Either way, a fix verified against a dev build is not live on the session
  until the binary the service points at is rebuilt and reinstalled.
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
  Streaming providers override it; one-shot query providers do not. It also has
  an optional `async fn snapshot(&self) -> Option<Value>` for one-shot
  `provider.query` reads; `QueryProviderUseCase` prefers it and falls back to
  the stream's first emission. Override `snapshot` when the provider can return
  its current state cheaply (notifications does).
- **Seed AND broadcast current state at startup; do not rely on events alone.**
  A streaming provider whose state is only updated by events shows a default
  (e.g. workspace `0`) until the first event arrives. Query the real initial
  state at construction (the hyprland active-window provider seeds the current
  workspace from `hyprctl monitors -j`'s `activeWorkspace`) AND send it on the
  broadcast channel so an already-subscribed frontend updates, not just late
  subscribers picking it up from `subscribe()`/`snapshot()`.
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
  — the first writable store in the project (the file explorer's pins store is
  the second); `ConfigStore` remains read-only TOML. Re-armed on startup via
  `TimerService::load_and_arm`.
  **A fired one-shot timer is `TimerStatus::Expired` immediately, and lingers.**
  When a one-shot fires, `TimerService::fire` sets `status = TimerStatus::Expired`
  (`src/application/src/use_cases/timer_service.rs`) and keeps it in the store
  (schedule-removal after a delay, or until dismissed); only *recurring* timers
  re-arm back to `Active`. So any UI that wants to show a "fired" state must key on
  the `Expired` timer still present in the `timers` array, NOT on an `Active` timer
  whose remaining time hit zero — that active-at-zero window is sub-second and you
  will almost never see it. The bar timer ring learned this the hard way: its
  `ringTarget()` (`src/ui/plugins/bar/views/bar/src/lib/tray/soonest.ts`) uses
  **fired-wins precedence** — a present `Expired` timer takes priority over any
  still-counting `Active` one, so a fired timer is never missed. The bar
  `TimerIndicator` renders the soonest active timer as a **fixed-size draining
  ring** (stopwatch icon when the store is empty, ring when non-empty, same box so
  the bar button never resizes) and derives everything client-side from
  `createTimerStore` (`timer.event`), ticking locally each second — no extra IPC.
- **Files subsystem (file explorer).** The `files` plugin view is the first
  `kind = "panel"` view: a normal decorated, resizable xdg-toplevel (no
  layer-shell), titled from its canonical name via `panel_title` in
  `src/ui/host/src/windows/panel.rs`. Its filesystem input/output lives in the
  `quantum-files` infrastructure crate (a seventh-plus sibling) behind domain
  ports `FileSystemPort` / `DirectoryWatcher` / `FileOpener` / `RecursiveSizer`
  (plus `PinsPort` and `ApplicationCatalog`), wired through `FilesService` in
  `application` and exposed as `files.*` dispatcher methods: `files.list`,
  `files.places`, `files.pin`, `files.unpin`, `files.operation`, `files.open`,
  `files.open_with`, `files.applications`, `files.open_terminal`,
  `files.preview`, `files.search`, `files.watch`, `files.unwatch`, `files.sizes`,
  `files.cancel_sizes`. Asynchronous updates publish on the `files.event`
  channel as a union discriminated by `event`: `{event:"changed", path}`,
  `{event:"size", path, bytes, complete}`, `{event:"operation_complete",
  operation}`, `{event:"operation_failed", message}`. Watches and recursive
  sizes are started on demand by `files.watch` / `files.sizes` — the subsystem
  needs no pre-subscription (unlike streaming providers). Pinned folders persist
  to a writable JSON store at `$XDG_STATE_HOME/quantum/files.json` (atomic
  temp-file plus rename, the timer-store pattern). The UI performs no filesystem
  input/output — it only speaks `files.*` IPC through `@quantum/client`.
  **SVG previews are inlined, not rasterized.** `files.preview` routes every
  `ContentKind::Image` through `read_image_preview`, which uses the `image` crate
  (`image::open`). That crate decodes only raster formats (png/jpeg/gif/webp), so
  an SVG returned `Unsupported` and the preview pane stayed blank. `read_image_
  preview_blocking` (`src/infrastructure/files/src/filesystem.rs`) now detects the
  `.svg` extension and inlines the markup verbatim as an `image/svg+xml;base64`
  data URI (capped at 1 MiB) — WebKit renders it natively, and `PreviewPane`
  already drops any `PreviewKind::Image` payload into `<img src>`. Key on the
  EXTENSION, not "is this valid XML", so a plain `.txt` still correctly fails.
  **`FilePreferences` (the `files.get_preferences`/`set_preferences` JSON store)
  carries `pinned_actions: Vec<PinnedAction { desktop_id, label }>`** — user-
  configured "Open with <app>" items rendered at the TOP of the file, folder, and
  background right-click menus, dispatched through the existing `files.open_with`
  (which `gio launch <desktop> <path>`es a file OR directory). Config-file-only:
  there is no in-app UI to add them; the user hand-edits `files.json`. When
  writing preferences the frontend must send the WHOLE object (`show_hidden` AND
  `pinned_actions`) — a partial `{ show_hidden }` write clobbers the pinned list.
- **Recursive folder sizing — parallel walk, mtime cache, self-removing
  handles.** `files.sizes(path)` sizes each immediate CHILD directory of `path`
  and streams `{event:"size", path:<child>, bytes, complete}` updates; the `files`
  view shows the running number, a `--color-accent` "Calculating" dot before it
  until `complete`, and re-requests sizes after a `changed` reload. Four
  invariants a future change MUST preserve (each was a real bug fixed 2026-07-20):
  - **`BackgroundSizer` walks cache-miss children CONCURRENTLY** on a bounded pool
    of `SIZER_WORKERS = 4` threads (`std::thread::scope`, stdlib, no crate). A huge
    child (a 253 GB Steam library) must not block its siblings — they size in
    parallel. `thread::scope` joins all workers before `walk_and_emit` returns, so
    the stream ends correctly. Cache HITS are served inline/serial (cheap).
  - **`FilesService.sizes` removes a walk's handle when the walk COMPLETES**, not
    just on `cancel_sizes` reaching count 0. Each walk gets a `generation`; the
    forwarding task removes its own `size_handles` entry on stream end only if the
    generation still matches. Without this, re-opening a folder found a dead
    ref-counted handle and started no new walk (folders stuck at their shallow
    ~4 KB inode size). `size_handles` is `Arc<Mutex<..>>` so the task can self-remove.
  - **The sizer caches child sizes** in a bounded LRU (`SizeCache`, cap
    `MAX_CACHE_ENTRIES = 4096`) keyed by child path + directory mtime; an
    mtime-match serves instantly with no walk. A COMPLETED walk populates the
    cache; a cancelled one does not. Invalidation is mtime-only, so a deep
    in-place edit (no add/remove at the child's top level) is NOT detected until
    the child's direct contents change — an accepted limitation.
  - **Symlinks are never followed** (a symlinked directory has `is_dir() == false`
    under `symlink_metadata`, so it is skipped). Folders whose real content lives
    behind symlinks (e.g. `~/.steam` → `~/.local/share/Steam`) therefore size only
    their tiny real content. This is deliberate (avoids cycles + double-counting),
    matching most file managers; do not "fix" it without cycle detection.
  Frontend note: `size` events are buffered and applied on a next-tick flush
  (`scheduleSizeFlush`), so the size-sorted list sorts once per batch rather than
  once per event — this smooths cached re-entry. A COLD walk's sizes still arrive
  seconds apart (one folder finishes at a time), so a size-sorted cold walk still
  reshuffles as they land; that is accepted, not a bug.
- **Processes subsystem (task manager).** Process sampling, correlation, and
  signalling live in the `quantum-processes` infrastructure crate: a procfs
  sampler that reads `/proc` for per-process and global statistics, a Hyprland
  client correlation that maps process ids to their owning windows, a gated
  one-hertz monitor (`TokioProcessMonitor`) that only samples while at least one
  watcher is listening, and a libc subtree killer (`LibcProcessKiller`) that
  resolves a target's process subtree from the monitor's freshest snapshot and
  delivers a signal to every member — refusing to signal quantumd's own process
  tree so killing a process from the panel can never take the daemon down. The
  domain ports are `ProcessMonitor` (streams snapshots) and `ProcessKiller`
  (`kill_subtree(pid, signal)`); `ProcessesService` in `application` wires them
  to the `EventBus`, turning frontend requests into port calls. Dispatcher IPC
  methods are `processes.watch` / `processes.unwatch` / `processes.kill`, the
  last taking `{ pid, signal: "term"|"kill" }` (`KillSignal` serializes
  lowercase). Streaming snapshots publish on the `processes.event` channel,
  each carrying a serialized `ProcessSnapshot`. The subsystem is entirely
  on-demand: there is no pre-subscription (unlike streaming providers),
  `processes.watch` / `processes.unwatch` are reference-counted, and the monitor
  does zero sampling while the panel is closed. The frontend is the
  `task-manager` panel plugin (a `kind = "panel"` view like the file explorer),
  opened from the bar `SystemMeters` right-click "Open Task Manager" menu item
  and the `SUPER+ESCAPE` Hyprland keybind.
- **Launcher power-up providers (calc, emoji, clipboard).** Three query
  providers back the launcher's prefix modes (parsed in the launcher view's
  `prefixMode.ts`; the prefix pins the provider like `>`/`!` pin `shell`):
  `=` (or bare math) → `calc` (`evalexpr`-backed arithmetic plus offline unit
  conversion, no currency), `:` → `emoji` (a bundled `emoji_data.json` via
  `include_str!`), `;` → `clipboard`. All three emit a domain `Action::Copy
  { text }` (routed through the shared `ClipboardWriter` port, backed by
  `WlClipboardWriter` shelling to `wl-copy`) so a selected result copies without
  launching. Each result may also carry provider-declared secondary actions in
  `Match.actions` (`Vec<MenuAction>`), which the launcher renders in a Ctrl+K /
  Tab / right-click menu via `openContextMenu`.
- **Clipboard subsystem.** A stateful subsystem like timers/files. The
  `ClipboardStore` domain port is implemented by `FileClipboardStore`
  (`quantum-providers`): metadata in `$XDG_STATE_HOME/quantum/clipboard.json`
  (atomic temp+rename, the timer-store pattern), raw bytes as blobs in
  `$XDG_STATE_HOME/quantum/clipboard/<id>.bin`. Four invariants a future change
  MUST preserve: (1) **blob written BEFORE the JSON row** so a crash leaves at
  worst an orphan blob, never a dangling row; (2) `load` **GCs orphan blobs** and
  drops rows whose blob is missing; (3) entry-count AND total-blob-byte **caps**
  evict oldest, and an oversized single entry is skipped; (4) image entries store
  a small inline PNG thumbnail data URI (reusing the file explorer's decode/
  resize path) so the launcher list renders with no per-row blob decode. The
  `ClipboardWatcher` shells to `wl-paste --watch` (command overridable via
  `[commands] clipboard_watcher`, else probed on PATH — the `lock_command`
  resolver pattern), classifies each change by MIME (`image/*`→image,
  `text/uri-list`→file, other `text/*`→text, else binary), and appends. The
  `clipboard` provider (`;` search, newest-first) recopies text via `write_text`
  and image/binary via `write_bytes` reading the blob; secondary actions delete
  or clear. `clipboard.clear` IPC routes through `ClipboardService` in
  `application` (holding the domain port — application never imports
  infrastructure). `wl-clipboard` is pinned in `shell.nix`.
- **Notifications: toast vs center.** A toast is the *transient* popup (it
  always auto-dismisses); the notification itself lives in the *center* until
  dismissed. `timeout_ms == 0` means "never expire" — it persists in the center
  but the toast still auto-dismisses. The provider resolves the D-Bus
  `expire_timeout` (`-1` server-default vs `0` never) and rejects remote
  (`http(s)://`) icon URLs before they reach the webview. `action.invoke`
  commands on the `notifications` provider: `dismiss` (one id), `action`
  (activate an action), `clear_toasts` (broadcasts `NotificationEvent::
  ToastsCleared` — display-only, store untouched; the bell sends it when opening
  the center). The store is capped (`MAX_NOTIFICATIONS`).
- **Completion sounds (`SoundPlayer`) depend on runtime PATH and file
  resolution — packaging owns half the fix.** `SoundPlayer::detect()`
  (`src/infrastructure/providers/src/timer_notifier.rs`) probes PATH for
  `canberra-gtk-play` (preferred) then `paplay`, and `play` is fire-and-forget
  (`let _ = ...spawn()`) — every failure is swallowed, so a broken sound is
  SILENT, never logged. Two ways it silently breaks, both hit on NixOS
  2026-08-05 (the notification chime AND the timer sound were inaudible):
  - **quantumd as a user service does NOT inherit the login shell's PATH.** If
    `canberra-gtk-play` is only in a user profile (home-manager) and not on the
    SERVICE's PATH, detection falls through to `paplay`. The daemon must be
    packaged with the player on its own PATH — the NixOS module wraps `quantumd`
    with `makeWrapper` to prepend `libcanberra-gtk3` (provides
    `canberra-gtk-play`; base `libcanberra` does NOT) + `pulseaudio` (`paplay`)
    and to add `sound-theme-freedesktop` to `XDG_DATA_DIRS`.
  - **The `paplay` fallback needs an absolute `.oga` path.** It does NOT resolve
    theme events itself (only `canberra-gtk-play` does). `resolve_sound_file`
    searches `XDG_DATA_DIRS` (then `/usr/share`, `/usr/local/share`) — never
    hardcode `/usr/share`, which does not exist on NixOS. When debugging "no
    sound", check the SERVICE's PATH (`/proc/<pid>/environ`) and whether the
    `.oga` file actually exists at the resolved path — not just that the code
    ran.
- **zbus deserialization: match the WIRE signature, and never swallow D-Bus
  errors silently.** `OwnedValue`/`Value` deserialize only a variant (`v`); a
  concrete struct out-argument must be deserialized as its concrete tuple type.
  com.canonical.dbusmenu `GetLayout` returns `u(ia{sv}av)` — deserializing as
  `(u32, OwnedValue)` (signature `uv`) fails with a signature mismatch on EVERY
  call, which left all tray menus permanently empty while three upstream
  "fixes" changed nothing. Use `RawMenuLayout` in `system_tray/menu.rs` (the
  signature is pinned by a unit test) and verify against the real bus with the
  ignored test `fetch_menu_deserializes_a_live_dbusmenu` (parametrized by
  `QUANTUM_TEST_MENU_BUS` / `QUANTUM_TEST_MENU_PATH`). The bug stayed invisible
  because the initial fetch did `Err(_) => return Vec::new()` — every D-Bus
  error path must at least `tracing::warn!`, and debugging any tray/D-Bus issue
  should start by checking the actual reply signature with
  `busctl --user call <bus> <path> <interface> <method> ...`.

- **Layer-shell usage differs by window type — `QUANTUM_LAYER_SHELL` gates
  only plain panels, not the bar.** `WidgetWindow` (the bar and the clock)
  always calls `init_layer_shell()` unconditionally (the two `WidgetWindow`
  constructors at `src/ui/host/src/windows/widget.rs:102` and `:240`).
  `PanelWindow` decides via
  `should_use_layer_shell(env_flag, is_overlay) = env_flag || is_overlay`
  (`should_use_layer_shell` at `src/ui/host/src/windows/panel.rs:46`): overlays (power-menu and similar)
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
- **A per-monitor bar must rebind to a FRESH `gdk::Monitor` when its monitor
  flaps — a warm hidden window keeps a stale binding.** Separate from the
  connector race above. The bar is a warm view (`destroy_on_dismiss == false`):
  `Close` hides and keeps it (`WindowRegistry::hide_window`), and it pins its
  layer-shell surface to a `gdk::Monitor` via `set_monitor()` at construction
  (`windows/widget.rs`). When a monitor disconnects/reconnects (a **flap**,
  common at plug-in — the multiplexer logs repeated open→close→reopen of
  `bar@<connector>`), GDK destroys and recreates that monitor's object, but the
  hidden warm window still holds `set_monitor(<old object>)`. On reshow the dead
  binding makes the compositor place the surface on the FOCUSED output, not the
  intended one → two bars on one monitor, none on the other, the orphan sized for
  the missing monitor. The registry rebuilds single-instance views when the
  requested monitor changes, but per-monitor views bypass that because the
  connector STRING ("DP-1") is unchanged across a flap — only the OBJECT identity
  changed. Fix (`registry.rs`): `WindowConstructor::monitor_identity(connector)`
  returns a `MonitorId` (GObject pointer identity); the registry stores it per
  window and, on Open of a suffixed per-monitor key whose window exists,
  evicts+reconstructs when the current identity differs from the stored one.
  Preserve this: any per-monitor warm window that pins a monitor must rebuild on
  object-identity change, not string change. Verify by replugging a second output
  and watching for `per-monitor view ...: monitor object changed, rebuilding
  window`, then `hyprctl layers` showing exactly one bar per monitor.
- All Vite views must set `base: './'` in `vite.config.ts` — the
  `quantum://` custom URI scheme breaks absolute URL normalization.
- Widget URLs are `quantum://theme/<theme>/views/widgets/<name>/index.html`.
  The `views/` prefix is required and is handled by `candidate_paths` in
  `src/infrastructure/theme-store/src/store.rs`.
- Always wrap window notify calls in
  `if (typeof window.__quantum_notify === 'function') { ... }` — events can
  arrive before the JS client has loaded.
- Svelte 5 components used in vitest must use `$effect` for setup work, not
  `onMount`. testing-library's legacy adapter does not fire `onMount`
  reliably under Svelte 5 runes mode.
- All Svelte views consume IPC through `@quantum/client`. Never reach into
  `window.__quantum_*` directly from view code.
- **WebKit's default browser context menu is suppressed on every view**
  (`suppress_browser_context_menu` in `src/ui/host/src/windows/mod.rs`, called
  from both WebView builders) — back/forward/reload are meaningless in a
  widget/launcher host. It is left in place only under `QUANTUM_INSPECTOR=1` so
  "Inspect Element" stays available. Views build right-click menus with the
  shared `openContextMenu` runtime in `@quantum/client`
  (`src/ui/packages/client/src/contextMenu.ts`): a themed DOM popover, viewport-
  clamped, dismissing on Escape/outside-click/scroll/blur, with optional
  `anchorRect` (drop down from a button), `onPlaced`, and `onClose`. Bar
  indicators wire it through `wireBarMenu`
  (`src/ui/plugins/bar/views/bar/src/lib/tray/barMenu.ts`), which handles the
  anchor and the bar input-region expand/reset (below).
- **Widget layer depends on interactivity. `Layer::Background` surfaces are
  NON-interactive in Hyprland — they receive no pointer clicks and no keyboard.**
  Content-sized widgets (the clock) sit on `Layer::Background` and anchor per the
  descriptor's `ViewPosition` (`Center` maps to top-right for the clock). A
  `fill_output = true` widget that needs interaction (the timers scatter
  surface: drag, the inline edit/dismiss controls, the edit form) sits on
  `Layer::Bottom` with `KeyboardMode::OnDemand` instead, so it still renders
  behind app windows but DOES receive clicks and keyboard. `fill_output` anchors
  all four edges to span the monitor (see `src/ui/host/src/windows/widget.rs`).
  If you add an interactive background widget, use Bottom + OnDemand, not
  Background/None.
- **Layer-shell surfaces capture pointer input across their whole region, even
  when transparent.** A mapped overlay/bottom surface swallows clicks over its
  area. The **bar** avoids this with input-region passthrough: it is a
  full-height transparent `Layer::Top` surface whose GdkSurface input region is
  clipped to the visible strip and expanded to cover an open dropdown via the
  `view.set_input_region` IPC (`WidgetWindow`, `src/ui/host/src/windows/widget.rs`);
  `wireBarMenu`'s `onPlaced`/`onClose` drive it. Surfaces without it still need
  the old workarounds: toasts are HIDDEN when empty via `view.hide`, and the
  full-screen timers surface captures bare-desktop clicks in its empty regions.
- **A full-height bar must anchor top/left/right only — never top AND bottom.**
  Anchoring opposite edges makes the compositor ignore the exclusive zone, so
  app windows tile *behind* the bar. The bar spans the monitor via
  `set_default_height(monitor_height)` with a single top-edge anchor while
  keeping `set_exclusive_zone(bar_height)` (`widget.rs`). It also runs on
  `KeyboardMode::OnDemand` so an open dropdown can take keyboard focus. (The
  `fill_output` timers surface is different: it anchors all four edges with
  exclusive zone 0.)
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
- **Overlay windows share one house style — match wifi-menu, do not invent.**
  Every first-party overlay (`kind = "overlay"`, `destroy_on_dismiss = true`)
  follows the wifi-menu shell. When adding one, clone that pattern exactly:
  - **No window-level close button.** No header `X`, no "Close" button.
    Overlays dismiss via **Escape + backdrop click ONLY** — a window keydown
    `$effect` handling `Escape`, and a `.backdrop` `onclick` guarded by
    `event.target === event.currentTarget`. Adding a close control is the most
    common inconsistency; do not.
  - **Bounded, scrolling body.** `.card` is `overflow: hidden` with a fixed
    `max-height: 80vh` and `width: min(420px, 92vw)` (420px is the norm — do not
    widen without reason); an inner `.scroll { overflow-y: auto; flex: 1 }` grows
    to fill. The card never grows unbounded.
  - **Card shell tokens:** `.backdrop` uses `var(--color-overlay-backdrop,
    rgba(0,0,0,0.5))` + `backdrop-filter: blur(4px)`; `.card` uses
    `var(--color-bg-alt)`, `border-radius: 12px`, `border: 1px solid
    var(--color-border)`, `box-shadow: 0 14px 40px var(--color-shadow)`.
  - **Dismissal:** `close()` calls `view.hide` by the **canonical** name
    `plugin/<plugin>/<view>` (never a legacy alias for a new plugin). Session-
    owning providers (wifi, bluetooth, audio) send `close_session` BEFORE
    `view.hide` — `destroy_on_dismiss` means the `$effect` cleanup is not a
    reliable place to stop a provider session.
  - **Theme tokens only — do not hardcode or invent tokens.** Backed by both
    themes: `--color-bg`, `--color-bg-alt`, `--color-border`, `--color-fg`,
    `--color-fg-alt`, `--color-accent`, `--color-muted`, `--color-surface`,
    `--color-overlay-backdrop`, `--color-shadow`, `--color-divider`,
    `--color-warning`, `--font-sans`. **Destructive/error color is
    `--color-error`** (theme-backed) — NOT `--color-danger`, `--color-bad`, or
    `--color-fg-muted`, which are undefined drift that always falls back to
    hardcoded hex. Active theme is `sycamore`.
  - **Icons:** no shared Icon package — `Icon.svelte` is copied per plugin
    (hand-drawn 24×24 `currentColor` SVG). Overlays use that SVG set or a Unicode
    entity glyph; **never emoji**. There are no headset/mouse/keyboard glyphs in
    the set — add SVG paths or use Unicode entities.
  - **Context menus / dropdowns:** build with `openContextMenu` from
    `@quantum/client`. `MenuItem` is `{ label, icon?, disabled?, danger?,
    separator?, onSelect }` — `icon` is a string glyph, `danger: true` renders
    red, and `options.anchorRect` drops the menu below a button (device-picker
    dropdown pattern). Separators are `{ separator: true }`.
- **`destroy_on_dismiss` teardown is fragile under rapid open/close plus async
  events.** Dismissing destroys the GTK window; a `spawn_local` future that
  later calls `gtk_window_destroy` on that already-gone surface aborts with
  `gdk_surface_get_display: assertion 'GDK_IS_SURFACE (surface)' failed`. Guard
  the destroy path against double-destroy / destroy-after-surface-gone
  (`src/ui/host/src/registry.rs`).

- **`ManagedWindow` enum dispatch: every `WindowOps` trait method MUST be
  forwarded.** `ManagedWindow` is an enum wrapping `PanelWindow` and
  `WidgetWindow`. Default trait method implementations are no-ops. When adding a
  new method to `WindowOps`, you must also add the dispatch match arm in
  `impl WindowOps for ManagedWindow` (`registry.rs:422`). Forgetting this leaves
  the method silently dead — the trait default runs, does nothing, and the inner
  window's implementation is never called. This bit `inject_view_args` on
  2026-08-21 and took multiple debugging cycles to identify.
- **File viewer (`plugin/file-viewer/file-viewer`).** A read-only file preview
  panel for Markdown, JSON, code, images, and video. Open from the shell with
  `qv <filepath>` (a wrapper around `quantumctl show plugin/file-viewer/file-viewer
  --args '{"path":"<absolute-path>"}'`). Markdown gets a collapsible TOC sidebar
  and rendered HTML; JSON gets brace/bracket folding and minified-file detection
  with a format prompt; code gets indentation-based folding and highlight.js
  syntax highlighting (15 languages). The viewer is a panel (`kind = "panel"`,
  `destroy_on_dismiss = true`), so it is rebuilt fresh on each open. When an agent
  wants to show the user a file for review (a plan, a config, a diff), open it
  with `qv` rather than dumping text in chat.
- **View arg passing: `window.__quantum_args`.** Views that accept arguments
  (file-viewer, and any future view that takes open-time parameters) read
  `(window as any).__quantum_args` in their Svelte `onMount`. The value is
  injected by `inject_view_args()` (`src/ui/host/src/windows/mod.rs`) both
  immediately and on `LoadEvent::Committed`. The canonical IPC call is
  `view.show` with params `{ "name": "<view>", "args": { ... } }`. CLI:
  `quantumctl show <view> --args '<json>'`.
- **Panel views need `#app { height: 100% }` for flex scrolling.** The mount
  point div must have explicit height for `overflow-y: auto` to work on flex
  children with `min-height: 0`. Without it, content overflows without scrolling.

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
