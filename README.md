# Quantum

A Wayland/Hyprland native launcher and widget host built with Rust and Svelte.

## Overview

Quantum is a daemon that provides a modern launcher interface for Linux desktop environments. It features:

- **Native Hyprland integration** — Direct IPC for window focusing and management
- **Search across multiple providers** — Applications, shell commands, custom scripts
- **Themeable views** — Svelte 5 frontends served via custom `quantum://` URI scheme
- **IPC-driven** — Control via `quantumctl` CLI or programmatically

For architecture details, see [Design Doc](docs/plans/2026-05-26-quantum-launcher-design.md).

## Working Agreement

See [AGENTS.md](AGENTS.md) for conventions, layer rules, and commit style.

## Quick Start

### Commands

| Command | Purpose |
|---------|---------|
| `just build` | Build all crates |
| `just test` | Run all tests |
| `just fmt` | Format Rust code |
| `just lint` | Clippy + warnings as errors |
| `just dev` | Run daemon in dev mode |
| `just ts-bindgen` | Regenerate TypeScript types |

### Building

```bash
just build
```

### Testing

```bash
just test
```

## Architecture

Quantum follows a strict onion architecture enforced at the Cargo crate level:

- **Domain** — Pure value objects, errors, and port definitions (no I/O)
- **Application** — Use cases and business logic
- **Infrastructure** — Implementations: providers, IPC, shell, config
- **UI** — GTK4 + WebKitGTK windows and Svelte frontends

See [AGENTS.md](AGENTS.md) for dependency rules.
