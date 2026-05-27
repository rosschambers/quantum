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
| `crates/domain`                | nothing (only `thiserror`, `serde`, `serde_json`, `async-trait`)        |
| `crates/application`           | `domain`                                   |
| `crates/infrastructure`        | `domain`                                   |
| `crates/ui`                    | `application` (and through it, `domain`)   |
| `crates/bin/quantumd`          | `ui`, `application`, `infrastructure`      |
| `crates/bin/quantumctl`        | `domain`, `infrastructure` (ipc client)    |
| `crates/bin/quantum-dev`       | `domain`, `infrastructure`                 |

**Forbidden:**
- `domain` importing any other workspace crate or any async/IO crate.
- `application` importing `infrastructure` or `ui`.
- `infrastructure` importing `application` or `ui`.
- `ui` importing `infrastructure` directly (must go through `application`).

A CI test in `tests/architecture.rs` enforces these rules by parsing Cargo
metadata. Do not weaken it.

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

## Rules

- No `unwrap`/`expect` outside tests and `main` setup.
- Errors are typed per-layer. Never leak Rust types across IPC.
- TDD. Write the failing test first.
- No emojis in source or commits.
- Verify before claiming done — run the actual command, read the actual output.
