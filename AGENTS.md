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
| `just ts-bindgen` | Regenerate TS types from Rust DTOs     |

## Build Environment

- **All builds, tests, lint, and format checks run via `./scripts/devsh.sh
  <command>`, which is a thin nix-shell wrapper around `shell.nix`.** The
  host's nixpkgs provides Rust 1.85, GTK4, WebKitGTK 6, and gtk4-layer-shell.
- The audio provider currently shells out to `pactl`, so that binary must be
  available in the host environment.

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

## UI and Frontend Conventions

- **Layer-shell is opt-in via `QUANTUM_LAYER_SHELL=1`.** Default is a normal
  xdg-toplevel so a stuck launcher window cannot lock the user out. The bar
  widget's WidgetWindow specializes layer-shell config (Layer::Top, anchor
  T/L/R, exclusive zone) only when the env flag is set.
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
