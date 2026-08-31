# Quantum — Ideas Backlog

Future additions to the bar / quantum. Not yet designed. Brainstorm each
before building (see the brainstorming skill).

## Shipped 2026-07-21 — launcher power-ups + clock calendar

Delivered: a
`calc` provider (`=` prefix and bare math, plus unit conversion), an `emoji`
provider (`:` prefix), a clipboard-history subsystem (`;` prefix, text + image
thumbnails + binary icons, blob-plus-JSON store, configurable `wl-paste`
watcher), a launcher secondary-actions panel (Ctrl+K / Tab / right-click,
driven by provider-declared `Match.actions`), and the clock calendar dropdown.

## Follow-up — make more shell-outs configurable

The clipboard work added a `[commands]` config section (following the
`lock_command` template) for `clipboard_watcher` and `clipboard_copy`. An audit
found other hardcoded shell-outs that could join the same section: `pactl`
(audio, 10 sites), `nmcli` (wifi), `xdg-open` + terminal + `gio`/`tar`/`unzip`
(files), the timer sound player (`canberra-gtk-play`/`paplay`), and `hyprctl`
(active-window seed). The terminal override is already threaded in code
(`ProcessFileOpener` accepts `terminal_override`) but hardwired to `None` at
`main.rs` — the lowest-effort win.

## Clipboard follow-up (phases 2/3, deferred)

Per-MIME glyphs for `text/uri-list` file copies, byte-budget eviction
refinements, a full-size `quantum://clip/<id>` preview view, and binary
paste-back via a persistent `wl-copy` offer.

## System tray (StatusNotifierItem)

An SNI host so third-party apps (Discord, Steam, nm-applet, and similar) can
show their tray icons in the bar. There is currently no SNI support at all —
the `tray/` folder in the bar holds quantum's own status indicators, not a
real application tray. Highest-impact gap after workspaces. Larger effort:
needs a DBus StatusNotifierHost/Watcher in infrastructure plus a bar widget.

## Batch of small bar widgets

Quick wins, each a small provider plus a bar indicator:

- Keyboard layout indicator (Hyprland exposes layout/switch events).
- Microphone mute toggle (audio provider already exists — add a mic source).
- Idle inhibitor / caffeine toggle.
- Network throughput (up/down bytes per second from the network provider).
- ~~Calendar popup when clicking the bar clock.~~ Done (2026-07-21): the bar clock
  opens a client-side month calendar dropdown (`CalendarDropdown.svelte`).

## Quick-settings / control-center panel

One popover bundling toggles and sliders that already have providers: wifi,
bluetooth, brightness, volume, power profile. Consolidates several existing
menus into a single control center.

## Weather widget

Bar indicator plus optional popover with current conditions and forecast.
Needs a weather provider (location + data source to be decided).

## Notification bugs (investigated 2026-06-17, not yet fixed)

### Bug 1 — toast popups vanish too fast (confirmed source defect)

- `apply_notify` collapses DBus `expire_timeout` `-1` ("server default") and
  `0` ("never expire") into the same `0`
  (`src/infrastructure/providers/src/notifications.rs:60`). The toast then
  treats `0` as "use 5s default"
  (`src/ui/plugins/notification-center/views/toast/src/App.svelte:42`) — the
  inverse of the domain's documented "0 = never auto-dismiss"
  (`src/domain/src/notifications.rs:20`). Persistent/critical notifications get
  auto-killed after 5 seconds.
- Positive timeouts are honored literally with no minimum floor, so apps
  sending 2-3s produce blink-and-miss toasts. No urgency-based persistence.
- `plugin.toml` advertises `default_timeout = 5000` but the view never reads it
  (hardcoded mirror) — a dead setting.
- Fix sketch: distinguish `-1` from `0` in the provider; in the toast treat
  `0` as never-dismiss, add a minimum visible floor, skip auto-dismiss for
  critical urgency, and read the real `default_timeout` from `plugin.toml`.
  Existing tests encode the buggy behavior and would need updating.

### Bug 2 — notification center list empty (likely stale deployed build)

- Storage/retrieval chain is correct at HEAD (fixed in commit `9bf71d9`).
  Notifications are stored in-memory; provider emits a catch-up snapshot on
  subscribe; the center calls `provider.query` for immediate catch-up.
- The live daemon is the nix-installed systemd service, not the dev build, and
  views are embedded at compile time from `dist/`. A daemon predating
  `9bf71d9` (or a `quantumd` not rebuilt after the center `dist/` regenerated)
  produces the "bell shows a count, center empty" split.
- Secondary fragility: the center's single `provider.query` catch-up swallows
  failures with an empty `.catch(() => {})` and never retries
  (`src/ui/packages/client/src/notifications.ts:69`), with a 2s timeout.
- First step: `just frontend-build` then rebuild/reinstall `quantumd`; verify
  with `RUST_LOG=info` that `notifications` is registered and pre-subscribed.
