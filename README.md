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

## Installation

### System Dependencies

On Ubuntu 24.04 or similar:

```bash
sudo apt-get install libgtk-4-dev libwebkit2gtk-6.0-dev libgtk4-layer-shell-dev
```

### Building from Source

```bash
just build
```

Then install:

```bash
cargo install --path src/binaries/quantumd
cargo install --path src/binaries/quantumctl
```

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

### Running

Build via `just build`:

```bash
just build
```

Or build inside the container and run on the host via `nix-shell`:

```bash
nix-shell --run "cargo build --bin quantumd --bin quantumctl"
```

Start the daemon in dev mode via `just`:

```bash
just dev
```

Or run directly via `nix-shell`:

```bash
RUST_LOG=info nix-shell --run "./target/debug/quantumd" &
```

Control the launcher via `quantumctl`:

```bash
quantumctl toggle launcher    # Show/hide
quantumctl show launcher      # Show
quantumctl hide launcher      # Hide
```

### Smoke Test Scripts

Manual verification scripts for end-to-end testing:

```bash
./scripts/manual-smoke-launcher.sh  # Test launcher window: type, enter, esc
./scripts/manual-smoke-widget.sh    # Test widget window: clock display
```

### Keybind Setup

Add the launcher to your Hyprland config:

```conf
bind = SUPER, SPACE, exec, quantumctl toggle launcher
```

See [packaging/hyprland/example.conf](packaging/hyprland/example.conf) for additional window rules and widget keybinds.

### Systemd Setup

To run the daemon as a user service, copy the unit file and enable it:

```bash
mkdir -p ~/.config/systemd/user
cp packaging/systemd/quantum.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now quantum
```

See [packaging/systemd/quantum.service](packaging/systemd/quantum.service) for details.

## Architecture

Quantum follows a strict onion architecture enforced at the Cargo crate level:

- **Domain** — Pure value objects, errors, and port definitions (no I/O)
- **Application** — Use cases and business logic
- **Infrastructure** — Implementations: providers, IPC, shell, config
- **UI** — GTK4 + WebKitGTK windows and Svelte frontends

See [AGENTS.md](AGENTS.md) for dependency rules.

## Documentation

- [Design Document](docs/plans/2026-05-26-quantum-launcher-design.md) — Full technical vision and design decisions
- [GUI Realization Plan](docs/plans/2026-05-28-quantum-gui-realization.md) — GTK4 + WebKit integration, threading model
- [Architecture](docs/architecture.md) — Onion layer structure, module organization, and threading model
- [Verification](docs/VERIFICATION.md) — v1 acceptance criteria checklist
- [Protocol Reference](docs/protocol.md) — IPC methods and message formats
- [Theming Guide](docs/theming.md) — How to write and customize themes
