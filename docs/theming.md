# Theming Guide

Quantum themes are a collection of TOML configuration files and Svelte 5 view bundles. Each theme defines design tokens (colors, fonts, spacing) and provides HTML/CSS/JS implementations for views like the launcher.

## Theme Structure

A minimal theme lives in `src/ui/themes/<name>/`:

```
src/ui/themes/my-theme/
├── theme.toml                      # manifest
├── tokens.toml                     # design token definitions
└── views/
    └── launcher/                   # one view per directory
        ├── package.json
        ├── vite.config.ts
        ├── index.html
        ├── src/
        │   ├── main.ts
        │   └── App.svelte
        └── dist/                   # built output (generated)
            ├── index.html
            └── assets/
```

---

## Theme Manifest (theme.toml)

Declare the theme's name, extends chain, and content sources.

```toml
[theme]
name = "my-theme"
version = "1.0.0"
description = "A custom theme"

# Extends another theme's tokens and views (optional)
extends = "default"                 # chain depth max = 8

# Token source
[tokens]
source = "file"                     # "file" or "inline"

# View definitions
[[views]]
name = "launcher"
description = "Application launcher"
```

### Cascade and Inheritance

A theme can extend another theme, creating a cascade:

```toml
extends = "default"
```

When resolving tokens:
1. Start with the root theme (default)
2. Walk the `extends` chain (with cycle detection)
3. Apply overrides in dependency order (root → leaf)

**Example cascade:**

```
my-theme → extends: dark-mode → extends: default
```

Resolution order:
1. Load default tokens
2. Merge dark-mode tokens
3. Merge my-theme tokens
```

---

## Token Definitions (tokens.toml)

Define design tokens as flat key-value pairs.

```toml
# Colors
color-bg = "#0a0e27"
color-fg = "#ffffff"
color-fg-muted = "#888888"
color-accent = "#6366f1"
color-border = "#333333"

# Typography
font-sans = "system-ui, sans-serif"
font-mono = "monospace"
font-size-base = "14px"
font-size-lg = "16px"
font-size-sm = "12px"

# Spacing (use a scale)
space-1 = "4px"
space-2 = "8px"
space-3 = "12px"
space-4 = "16px"
space-5 = "20px"
space-6 = "24px"

# Radii
radius-sm = "2px"
radius-md = "4px"
radius-lg = "8px"

# Motion
duration-fast = "150ms"
duration-base = "300ms"
ease-default = "cubic-bezier(0.4, 0, 0.2, 1)"
```

Token names follow kebab-case convention. The daemon injects them as CSS custom properties at runtime:

```css
background: var(--color-bg);
color: var(--color-fg);
border-radius: var(--radius-md);
```

---

## View Bundles

Each view (e.g., launcher) is a self-contained Vite project that builds to a static bundle.

### Vite Configuration

```typescript
// vite.config.ts
import { defineConfig } from 'vite'
import { svelte } from 'vite-plugin-svelte'

export default defineConfig({
  plugins: [svelte()],
  base: 'quantum://theme/my-theme/views/launcher/',
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    rollupOptions: {
      output: {
        entryFileNames: 'assets/[name].js',
        chunkFileNames: 'assets/[name].js',
        assetFileNames: 'assets/[name][extname]'
      }
    }
  }
})
```

### HTML Template

```html
<!-- index.html -->
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Quantum Launcher</title>
  <style id="quantum-tokens"></style>
  <link rel="stylesheet" href="styles.css">
</head>
<body>
  <div id="app"></div>
  <script type="module" src="/src/main.ts"></script>
</body>
</html>
```

The `<style id="quantum-tokens"></style>` element is populated by the daemon with CSS custom property declarations.

### Svelte Component Example

```svelte
<!-- src/App.svelte -->
<script lang="ts">
  import { createClient } from '@quantum/client'
  
  const client = createClient()
  let query = ''
  let results = []
  let selectedIndex = 0
  
  async function search() {
    const { matches, warnings } = await client.call('search', {
      text: query,
      providers: [],
      limit: 20
    })
    results = matches
    selectedIndex = 0
  }
  
  async function invoke() {
    if (results[selectedIndex]) {
      const match = results[selectedIndex]
      await client.call('action.invoke', {
        provider_id: match.provider,
        action: match.action
      })
      // Close launcher
      await client.call('view.hide', { name: 'launcher' })
    }
  }
  
  function handleKeydown(e) {
    if (e.key === 'ArrowDown') {
      selectedIndex = (selectedIndex + 1) % results.length
    } else if (e.key === 'ArrowUp') {
      selectedIndex = (selectedIndex - 1 + results.length) % results.length
    } else if (e.key === 'Enter') {
      invoke()
    } else if (e.key === 'Escape') {
      client.call('view.hide', { name: 'launcher' })
    }
  }
</script>

<input
  type="text"
  placeholder="Search..."
  bind:value={query}
  on:input={search}
  on:keydown={handleKeydown}
  autofocus
/>

<ul>
  {#each results as result, i}
    <li class:selected={i === selectedIndex}>
      {result.title}
      {#if result.subtitle}
        <small>{result.subtitle}</small>
      {/if}
    </li>
  {/each}
</ul>

<style>
  :global(body) {
    background: var(--color-bg);
    color: var(--color-fg);
    font-family: var(--font-sans);
  }
  
  input {
    width: 100%;
    padding: var(--space-3);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
  }
  
  ul {
    list-style: none;
    padding: 0;
  }
  
  li {
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius-sm);
  }
  
  li.selected {
    background: var(--color-accent);
    color: white;
  }
</style>
```

---

## Building a View Bundle

```bash
cd src/ui/themes/my-theme/views/launcher
pnpm build
```

Output goes to `dist/`, with `index.html` and hashed asset imports.

---

## Hot Reload During Development

While developing, use `quantum-dev watch`:

```bash
quantum-dev watch
```

This watches `src/ui/themes/` and pushes `theme.reload` to the running daemon whenever files change. The frontend reloads the view.

---

## Built-in Default Theme

The `default` theme is embedded in the binary via `include_dir!`. Its structure:

```
src/ui/themes/default/
├── theme.toml              (name="default", no extends)
├── tokens.toml             (standard Quantum design tokens)
└── views/launcher/
    └── dist/               (pre-built Svelte bundle)
```

Builds happen at daemon compile time:

```bash
# In src/infrastructure/build.rs
cargo run -C ../../src/ui/themes/default/views/launcher build
# Then include_dir! packs the `dist/` folder
```

---

## Extending the Default Theme

Create a new theme that inherits from default and overrides tokens:

```toml
# src/ui/themes/dark/theme.toml
[theme]
name = "dark"
extends = "default"

[tokens]
source = "file"
```

```toml
# src/ui/themes/dark/tokens.toml
# Only override what you need
color-bg = "#0a0a0a"
color-fg = "#ffffff"
color-accent = "#ff6b9d"
```

The daemon merges these over the default tokens.

---

## Custom Views

Create a new view (e.g., widgets) by adding to a theme:

```toml
[[views]]
name = "widgets"
description = "Floating widgets"
```

Then implement `src/ui/themes/<name>/views/widgets/`:

```bash
mkdir -p src/ui/themes/my-theme/views/widgets
cd src/ui/themes/my-theme/views/widgets

# Copy structure from launcher
cp -r ../launcher/* .

# Edit App.svelte to show widgets instead
```

Build and activate via `quantumctl show widgets`.

---

## Writing a custom widget

Custom widgets are pure HTML/CSS/JS bundles served from a theme directory.
They subscribe to the same providers as built-in widgets through the
`window.__quantum_notify` global or, more conveniently, through
`@quantum/client`.

### Minimal example

See `examples/widgets/cpu-test/index.html` for a pure-HTML widget that
displays the current CPU usage. To install it:

1. Copy the folder to your user theme:
   ```bash
   mkdir -p ~/.config/quantum/themes/default/views/widgets/cpu-test
   cp examples/widgets/cpu-test/index.html \
      ~/.config/quantum/themes/default/views/widgets/cpu-test/
   ```
2. Tell the daemon about it in `~/.config/quantum/config.toml`:
   ```toml
   [[widget]]
   view = "widgets/cpu-test"
   auto_show = true
   ```
3. Restart quantumd. The widget appears at top-right of the screen and
   updates once per second.

### Provider channels you can subscribe to

| Channel                          | Payload type           | Update cadence              |
| -------------------------------- | ---------------------- | --------------------------- |
| `system.stats.event`             | `SystemStats`          | 1 Hz                        |
| `mpris.event`                    | `MprisState`           | On DBus signal or 1 Hz poll |
| `hyprland.activewindow.event`    | `ActiveWindowState`    | On Hyprland event push      |
| `power.event`                    | `PowerState`           | On UPower PropertiesChanged |
| `network.event`                  | `NetworkState`         | On NM PropertiesChanged     |
| `bluetooth.event`                | `BluetoothState`       | On BlueZ ObjectManager      |
| `power_profile.event`            | `PowerProfileState`    | On PPD PropertiesChanged    |
| `audio.event`                    | `AudioState`           | 200 ms poll via pactl       |
| `brightness.event`               | `BrightnessState`      | 1 Hz sysfs poll             |
| `system_power.event`             | `SystemPowerState`     | One-shot capability snapshot|

Every tray payload has a top-level `available: boolean`. When `false` the
rest of the fields are `null` / empty / default — the backing service
(UPower, NetworkManager, BlueZ, etc.) is not present on this host.
Indicator widgets should render nothing in that case. The provider keeps
the stream alive and re-emits once the service appears via
`NameOwnerChanged`, so plugging in a Bluetooth dongle later transitions
the bar from empty to populated without a daemon restart.

`system_power` is action-only: it emits its capability flags once on
subscribe and then the stream stays pending. The capability flags
(`can_shutdown`, `can_restart`, `can_suspend`, `can_hibernate`,
`can_lock`) reflect what logind reports for the current session at
daemon startup; they do not change at runtime because polkit rules
don't reload on a running daemon.

### Invoking actions

Any widget can call dispatcher methods over the same WebKit bridge:

```js
window.webkit.messageHandlers.quantum.postMessage(JSON.stringify({
    jsonrpc: '2.0',
    id: 42,
    method: 'action.invoke',
    params: {
        provider: 'mpris',
        action: { kind: 'custom', data: { kind: 'mpris', payload: { command: 'play-pause' } } }
    }
}));
```

Responses come back through `window.__quantum_resolve(id, value)`
and `window.__quantum_reject(id, error)`.

### Using @quantum/client

For TypeScript widgets with bundling (Vite, etc.), use the
`@quantum/client` package which provides a typed API around the bridge.
See `src/ui/themes/default/views/widgets/bar/` for a Svelte 5
example.

---

## Tray indicators

The default `widgets/bar` view ships with a `Tray` region containing six
indicators driven by the providers listed above. Each indicator is a
self-contained Svelte 5 component under
`src/ui/themes/default/views/widgets/bar/src/lib/tray/`. They are a
useful reference for building custom indicators in your own theme.

Each indicator follows the same pattern: subscribe to the channel,
ignore the payload when `available === false`, render a small glyph plus
tooltip otherwise, dispatch `action.invoke` on user interaction.

| Indicator              | Channel              | Interaction                                |
| ---------------------- | -------------------- | ------------------------------------------ |
| `BatteryIndicator`     | `power.event`        | Display only                               |
| `NetworkIndicator`     | `network.event`      | Left click toggles WiFi                    |
| `BluetoothIndicator`   | `bluetooth.event`    | Left click toggles adapter power           |
| `VolumeIndicator`      | `audio.event`        | Left click toggles mute; scroll = +/-5%    |
| `BrightnessIndicator`  | `brightness.event`   | Scroll = +/-5% on the first display        |
| `PowerProfileIndicator`| `power_profile.event`| Left click cycles to the next available    |
| `PowerMenuIndicator`   | `system_power.event` | Click opens the centered `widgets/power-menu` view |

### Custom indicator skeleton

```svelte
<script lang="ts">
    import type { Client } from '@quantum/client';
    import type { PowerState } from '../types';
    import { POWER_CHANNEL } from '../channels';

    interface Props { client: Client }
    let { client }: Props = $props();
    let state: PowerState = $state({
        available: false,
        on_battery: false,
        percentage: null,
        state: null,
        time_to_empty_secs: null,
        time_to_full_secs: null,
    });

    $effect(() => {
        const off = client.subscribe(POWER_CHANNEL, (p: unknown) => {
            state = p as PowerState;
        });
        return () => off?.();
    });
</script>

{#if state.available}
    <div class="tray-icon">{Math.round(state.percentage ?? 0)}%</div>
{/if}
```

### Action envelope

For provider-specific commands, wrap the payload in
`Action::Custom`:

```js
await client.call('action.invoke', {
    provider: 'audio',
    action: {
        kind: 'custom',
        data: { kind: 'audio', payload: { command: 'toggle_mute' } },
    },
});
```

The inner `data` carries the variant fields; the inner `payload` carries
the provider-specific command shape. See `docs/protocol.md` for the IPC
method and payload reference.

### system_power configuration

The `system_power` provider invokes a configurable lock command for the
`lock` action. Configure it in `~/.config/quantum/config.toml`:

```toml
[system_power]
lock_command = "hyprlock --grace 0"
```

The value is shell-tokenized (so `"hyprlock --grace 0"` becomes
`["hyprlock", "--grace", "0"]`). When unset the daemon probes
`hyprlock`, `swaylock`, `gtklock`, then falls back to
`loginctl lock-session`. The first that resolves on `$PATH` wins.

The five `system_power` commands are:

```js
// In a custom widget. provider id = 'system_power'.
{ kind: 'custom', data: { kind: 'system_power', payload: { command: 'shutdown' } } }
{ kind: 'custom', data: { kind: 'system_power', payload: { command: 'restart' } } }
{ kind: 'custom', data: { kind: 'system_power', payload: { command: 'suspend' } } }
{ kind: 'custom', data: { kind: 'system_power', payload: { command: 'hibernate' } } }
{ kind: 'custom', data: { kind: 'system_power', payload: { command: 'lock' } } }
```

Each command is gated on its `can_*` flag from the snapshot; invoking
when the flag is false returns an error. The first four go through
`org.freedesktop.login1.Manager.{PowerOff,Reboot,Suspend,Hibernate}(false)`
(with auth prompts disabled — polkit rules either allow it or fail
fast). `lock` spawns the resolved lock command detached.

---

## Power menu

The default theme ships a centered modal view at
`src/ui/themes/default/views/widgets/power-menu/` that the bar's
`PowerMenuIndicator` opens via `view.show widgets/power-menu`. Themes
override the look by providing the same path.

The view fetches `SystemPowerState` from the `system_power` provider
on mount and renders one row per available action. Each row uses a
two-click confirmation pattern (first click arms, second click within
3 seconds fires). A hidden-by-default "Schedule…" toggle next to each
row reveals preset pills (`Now`, `5m`, `15m`, `30m`, `1h`) and a
custom-minutes input (clamped 1–1440 minutes = 24 hours).

The view loads in a `PanelWindow` (the same window type as the
launcher), centered on screen with `KeyboardMode::Exclusive` while
visible. Escape, backdrop click, and the close button all dispatch
`view.hide widgets/power-menu`.

### Scheduled actions

Three IPC methods support the timer feature; all live on the
dispatcher and are reusable for any provider that exposes
`action.invoke`.

**`action.schedule`** — queue a delayed invocation.

```js
await client.call('action.schedule', {
    delay_secs: 1800,          // 30 minutes; must be > 0, <= 86400
    label: 'Suspend',          // displayed in the scheduled-jobs list
    action: {                  // the same { provider, action } envelope
        provider: 'system_power',
        action: {
            kind: 'custom',
            data: {
                kind: 'system_power',
                payload: { command: 'suspend' },
            },
        },
    },
});
// → { id: 'a3f7d2c1' }  — 8-char hex opaque id
```

**`action.cancel`** — abort a scheduled job by id.

```js
await client.call('action.cancel', { id: 'a3f7d2c1' });
// → {}
```

**`action.scheduled`** — list currently scheduled jobs.

```js
await client.call('action.scheduled', {});
// → { jobs: [{ id, fires_at, label }, ...] }
```

`fires_at` uses the default serde serialization of `SystemTime`:
`{ secs_since_epoch: u64, nanos_since_epoch: u32 }`. The frontend
computes "in N min" client-side from this.

**Persistence**: scheduled jobs are in-memory only. Daemon restart
drops the queue. The daemon logs each pending job at WARN on
shutdown so a user inspecting the log knows what was lost.

---

## Cycle Detection

If a theme's `extends` chain forms a cycle, the daemon logs an error and falls back to the default theme:

```
Error loading theme: cycle detected in extends chain (depth > 8)
Falling back to default theme
```

---

## Token Injection

When a view is loaded, the daemon injects CSS custom properties into the `<style id="quantum-tokens">` element:

```css
--color-bg: #0a0e27;
--color-fg: #ffffff;
--font-sans: system-ui, sans-serif;
--space-4: 16px;
...
```

The view's CSS references them:

```css
body {
  background: var(--color-bg);
  color: var(--color-fg);
  font-family: var(--font-sans);
  padding: var(--space-4);
}
```

---

## Troubleshooting

**Theme fails to load:**

```bash
# Check the log
RUST_LOG=quantum_infrastructure=debug quantumd --headless
```

**CSS variables not applied:**

- Ensure tokens are defined in `tokens.toml`
- Check the browser console for CSS variable errors
- Verify `<style id="quantum-tokens">` is populated

**Hot reload not working:**

```bash
quantum-dev watch --verbose
# Check that daemon is running and the socket is accessible
```

---

## Multi-monitor bars

The default theme's `widgets/bar` view is multi-monitor aware. On
startup the daemon installs a `BarMultiplexer` on the GTK thread that
watches `gdk::Display::monitors()` and spawns one bar window per
connected output. The window-registry key for each per-monitor bar is
`widgets/bar@<connector-name>`, where the connector name comes from
the Wayland output (matching what Hyprland reports in `hyprctl
monitors`). Examples: `widgets/bar@DP-1`, `widgets/bar@HDMI-A-1`,
`widgets/bar@eDP-1`.

You can target a specific bar through the existing IPC. For example,
to hide just the bar on `DP-1`:

```bash
quantumctl call view.hide '{"name":"widgets/bar@DP-1"}'
```

Each per-monitor bar has its monitor's connector name injected into
the WebView as a JavaScript global before the page commits its first
load:

```js
window.__quantum_monitor; // for example "DP-1"
```

Custom themes can read this global to scope per-monitor behaviour.
The default theme's `ActiveWindow.svelte` uses it to render the
focused window for that monitor specifically — the underlying
`hyprland.activewindow` payload carries state for every monitor
keyed by connector name, and the component picks its own.

When a monitor is hot-plugged the multiplexer spawns a new bar within
about one second (the latency of Hyprland's `monitoradded` event
followed by GTK's `items-changed` signal). When a monitor is
disconnected the corresponding bar is torn down and the layer-shell
surface released. Per-monitor opt-out (a bar-blacklist in
`config.toml`) is a planned follow-up but not yet implemented; if
`auto_show = true` is set for `widgets/bar` in config, every
connected monitor gets a bar.

---

## See Also

- [docs/protocol.md](protocol.md) — `theme.reload` method
- [docs/architecture.md](architecture.md) — ThemeStore cascade implementation
