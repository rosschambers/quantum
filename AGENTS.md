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
| `src/infrastructure/git`       | `domain`, sibling infrastructure crates    |
| `src/infrastructure/hyprland`  | `domain`, sibling infrastructure crates    |
| `src/infrastructure/ipc`       | `domain`, sibling infrastructure crates    |
| `src/infrastructure/plugins`   | `domain`, sibling infrastructure crates    |
| `src/infrastructure/providers` | `domain`, sibling infrastructure crates    |
| `src/infrastructure/theme-store` | `domain`, sibling infrastructure crates  |
| `src/ui/host`                  | `application` (and through it, `domain`)   |
| `src/binaries/quantumd`        | `ui`, `application`, `infrastructure`      |
| `src/binaries/quantumctl`      | `domain`, `infrastructure` (ipc client)    |
| `src/binaries/quantum-dev`     | `domain`, `infrastructure`                 |

Infrastructure is **eleven sibling crates**, not one. Sibling infrastructure
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
- **Every `git commit` runs the committed pre-commit hook**
  (`scripts/hooks/pre-commit`, wired by `just` via `core.hooksPath`): workspace
  `cargo fmt --check` plus `cargo clippy --workspace --all-targets -- -D
  warnings`, even for a frontend-only or docs-only commit. On a cold `target/` that
  is about 9 minutes; warm, about 35 seconds. Give an agent's `git commit` a tool
  timeout of 30 minutes. A 2-minute default kills the commit mid-hook, and nothing
  is committed (the staged files survive). ^(daily:2026-10-15)

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

## Provider, windowing & subsystem conventions — in `docs/subsystems.md`

The detailed conventions for providers, windows/layer-shell/views, and each subsystem are **not**
inline (they were ~9k tokens). Open `docs/subsystems.md` before touching any of: a `ProviderSource`
(streaming vs query, `subscribe`/`snapshot`, channel naming `<provider>.event`, register +
pre-subscribe in `main.rs`, seed-and-broadcast current state at startup); the bar / panels / overlays /
per-monitor windows (layer-shell gating `QUANTUM_LAYER_SHELL`, per-monitor monitor-object identity on
flap, exclusive-zone anchoring, input-region passthrough, webview-release-on-destroy, `ViewMultiplexer`
auto-open needs both `per_monitor` and `auto_show`); view args (`window.__quantum_args`) /
multi-instance (`#<id>` suffix, `max_instances` cap) / self-close (`window.__quantum_view_name`); the
`@quantum/client` bridge; context menus via `openContextMenu`; the file-viewer `qv` (markdown/mermaid/
diff/staging/sanitization/CSP).

Highest-risk cross-cutting rules, kept inline so they are never missed:
- **Theme tokens only** — use the `--color-*` / `--font-sans` tokens (active theme is `sycamore`); destructive/error color is `--color-error`, never `--color-danger`/`-bad`. Never hardcode hex.
- **Views speak IPC only through `@quantum/client`** — never reach into `window.__quantum_*` (sole exceptions: read `__quantum_args` and `__quantum_view_name`).
- **Overlays share one house style** (clone wifi-menu): no window close button, Escape + backdrop dismiss only, bounded scrolling body, theme tokens.
- **Every spawned external process must be reaped** (`spawn_and_reap`) — a dropped `std::process::Child` leaks a zombie per call under the long-lived daemon.
- **A user/daemon service does not inherit the login PATH** — wrap binaries that shell out to helpers (sound players, git) so the helper is on the *service's* PATH.

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
