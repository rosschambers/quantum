# Quantum

A desktop shell for Wayland. Launcher, status bar, notifications, and system menus — all in one daemon.

![Quantum launcher showing application search results](assets/launcher.png)

## What it does

Quantum runs as a single daemon on Hyprland. It gives you:

- **Launcher** with fuzzy search across apps, open windows, and shell commands. Prefix modes: `=` calculator with unit conversion, `:` emoji picker, `;` clipboard history with image thumbnails. Ctrl+K opens secondary actions on any result.
- **Status bar** spanning each monitor — workspaces, active window title, media controls, system tray, clock. Per-monitor: plug in a display, the bar appears.
- **Wi-Fi manager** — scan, connect, switch bands, see signal strength. No NetworkManager applet needed.
- **Sound mixer** — per-device volume sliders for every output and input, playback info, PipeWire/PulseAudio device profile switching.
- **Notifications** — D-Bus notification daemon with toast popups and a notification center.
- **Bluetooth manager** — pair, connect, forget, switch profiles.
- **Timers** — create, edit, dismiss. Persistent across restarts. Desktop ring indicator in the bar.
- **File explorer** — panel view with places sidebar, right-click context menus, recursive folder sizing, file previews (markdown, code, images, video).
- **File viewer** — open any file for read-only preview from the shell with `qv <path>`.
- **System tray** — full D-Bus system tray with nested menus (SNI + dbusmenu).
- **Clipboard manager** — searchable history with image support, accessible from the launcher.

### Viewer and explorer improvements (unreleased)

Implemented and locally verified; installation and verification in the running
desktop service remain separate deployment steps.

- **Find in file:** Ctrl+F (or Command+F) opens literal, case-insensitive search in
  text, code, JSON, and rendered Markdown. Enter / Shift+Enter move forward / back
  with wraparound and a match counter. Escape closes search before the viewer.
  Search reveals offscreen virtualized lines and expands containing folds without
  changing unrelated folds; revealed folds stay open after search closes.
  Markdown searches rendered text, including across inline formatting, not raw
  markup or diagram source/SVG. Regular expressions and replacement are not supported.
- **Images:** Fit, Actual size (natural dimensions in CSS pixels, not physical
  display pixels), zoom controls, and drag-to-pan when the image overflows.
  Load/decode failures show an error instead of a blank pane. PNG, JPEG, GIF, WebP,
  SVG, BMP, ICO, and AVIF loaded in the isolated native built-bundle verification;
  this is not a guarantee for every file or decoder installation.
- **Explorer:** the directory tree now virtualizes rows alongside the existing
  virtualized file list. Owner lookup reads `/etc/passwd` once per listing;
  concurrent same-path listings share work without caching settled results.
  Shared live updates remain, with stale pane-load results guarded during reload,
  navigation, and teardown. Residual large-directory delays remain under investigation.

See [development verification](docs/development.md#viewer-and-explorer-verification)
for the image security boundary, test commands, and native coverage limits.

## Screenshots

### Status bar

![Status bar spanning the full monitor width](assets/bar.png)

### Sound mixer

![Sound mixer with per-device output and input sliders](assets/sound.png)

### Bluetooth

![Bluetooth manager with connected and available devices](assets/bluetooth.png)

### File explorer

![Dual-pane file explorer showing /etc and a project directory](assets/file-explorer.png)

### File viewer

![File viewer rendering a markdown document with a table-of-contents sidebar](assets/file-viewer.png)

## Tech stack

Rust daemon (Tokio + GTK4 + WebKitGTK + gtk4-layer-shell) serving Svelte 5 frontends over a custom `quantum://` URI scheme. IPC via Unix socket. Controlled with `quantumctl`.

## Build and run

Builds run through a nix-shell (`shell.nix`) that provides the correct Rust toolchain, GTK4, WebKitGTK 6, and gtk4-layer-shell.

```bash
just build          # build all crates
just test           # run all tests
just lint           # clippy, warnings as errors
just fmt            # format rust code
just dev            # run the daemon in dev mode
```

Or directly through the nix-shell wrapper:

```bash
./scripts/devsh.sh cargo build --bin quantumd --bin quantumctl
```

### Install the binaries

```bash
./scripts/devsh.sh cargo install --path src/binaries/quantumd
./scripts/devsh.sh cargo install --path src/binaries/quantumctl
```

### Keybind

Add to your Hyprland config:

```conf
bind = SUPER, SPACE, exec, quantumctl toggle launcher
```

See [packaging/hyprland/example.conf](packaging/hyprland/example.conf) for window rules and widget keybinds.

### Systemd service

```bash
mkdir -p ~/.config/systemd/user
cp packaging/systemd/quantum.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now quantum
```

### Without nix (unsupported)

You will need GTK4, WebKitGTK 6, and gtk4-layer-shell development packages, plus a manual pkg-config alias (`gtk4-layer-shell.pc` pointing at `gtk4-layer-shell-0.pc`). The nix-shell handles this automatically.

## Architecture

Onion architecture enforced at the Cargo crate level:

- **Domain** — pure types, errors, port traits. No I/O.
- **Application** — use cases and business logic.
- **Infrastructure** — providers, IPC, D-Bus, Hyprland, config, files, processes.
- **UI** — GTK4 + WebKitGTK windows and Svelte frontends.

See [docs/architecture.md](docs/architecture.md) for the full layout.

## Documentation

- [Architecture](docs/architecture.md) — layers, modules, threading model
- [Protocol](docs/protocol.md) — IPC methods and message formats
- [Theming](docs/theming.md) — writing and customizing themes
- [Development](docs/development.md) — fast local iteration workflow

## License

MIT
