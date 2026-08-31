# Quantum

A Wayland/Hyprland native launcher and widget host built with Rust and Svelte.

## Overview

Quantum is a daemon that provides a modern launcher interface for Linux desktop environments. It features:

- **Native Hyprland integration** — Direct IPC for window focusing and management
- **Search across multiple providers** — Applications, open windows, shell commands, custom scripts
- **Launcher power-ups** — Inline calculator and unit conversion (`=`), emoji picker (`:`), and clipboard history with image thumbnails (`;`), plus a Ctrl+K secondary-actions panel
- **Themeable views** — Svelte 5 frontends served via custom `quantum://` URI scheme
- **IPC-driven** — Control via `quantumctl` CLI or programmatically

For architecture details, see [Architecture](docs/architecture.md).

## Working Agreement

See [AGENTS.md](AGENTS.md) for conventions, layer rules, and commit style.

## Installation

### Canonical: nix-shell

All builds, tests, lint, and format checks run through the nix-shell defined
by `shell.nix`, wrapped by `./scripts/devsh.sh`. This is the supported path:
it provides the correct Rust, GTK4, WebKitGTK 6, and gtk4-layer-shell, and
installs the pkg-config alias the `gtk4-layer-shell` crate needs (upstream
ships `gtk4-layer-shell-0.pc`, but the crate looks up `gtk4-layer-shell.pc`).

```bash
just build          # builds all crates inside the nix-shell
# or, equivalently:
./scripts/devsh.sh cargo build --bin quantumd --bin quantumctl
```

To install the built binaries for the systemd user service:

```bash
./scripts/devsh.sh cargo install --path src/binaries/quantumd
./scripts/devsh.sh cargo install --path src/binaries/quantumctl
```

### Advanced / unsupported: system toolchain

Building against a system-installed toolchain (outside nix-shell) is **not
supported**. If you attempt it, you must supply the GTK4, WebKitGTK 6, and
gtk4-layer-shell development packages yourself, for example on Ubuntu:

```bash
sudo apt-get install libgtk-4-dev libwebkit2gtk-6.0-dev libgtk4-layer-shell-dev
```

You must also alias the pkg-config file the `gtk4-layer-shell` crate expects
(`gtk4-layer-shell.pc` pointing at the system's `gtk4-layer-shell-0.pc`);
`shell.nix` does this automatically, a bare `cargo install` does not.

## Quick Start

### Commands

| Command | Purpose |
|---------|---------|
| `just build` | Build all crates |
| `just test` | Run all tests |
| `just fmt` | Format Rust code |
| `just lint` | Clippy + warnings as errors |
| `just dev` | Run daemon in dev mode |

### Running

Build via `just build`:

```bash
just build
```

Or build through the nix-shell wrapper directly:

```bash
./scripts/devsh.sh cargo build --bin quantumd --bin quantumctl
```

Start the daemon in dev mode via `just`:

```bash
just dev
```

Or run directly through the nix-shell wrapper:

```bash
RUST_LOG=info ./scripts/devsh.sh ./target/debug/quantumd &
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

- [Architecture](docs/architecture.md) — Onion layer structure, module organization, and threading model
- [Verification](docs/VERIFICATION.md) — v1 acceptance criteria checklist
- [Protocol Reference](docs/protocol.md) — IPC methods and message formats
- [Theming Guide](docs/theming.md) — How to write and customize themes
