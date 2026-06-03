# Plugins

Quantum lets you ship custom widgets without writing Rust. Drop a folder
under `~/.config/quantum/plugins/<name>/` containing some HTML, CSS, JS,
and (optionally) some shell scripts. Quantum auto-discovers everything
based on folder layout. No build step, no manifest, no Vite.

This guide walks through a complete moon-distance plugin from scratch.
By the end you will have a clickable widget showing data fetched by a
shell script.

## Folder layout

```
~/.config/quantum/plugins/
  moon-distance/                  <-- the plugin's name (any kebab-case)
    config.toml                   (optional - only needed for polling)
    scripts/
      moon-distance               (executable file, any language)
    actions/
      open-calendar               (executable file, any language)
    views/
      moon-widget/                <-- view name
        index.html                (required)
        style.css                 (optional)
        script.js                 (optional)
```

The four special folders are recognised by name:

- `scripts/` - polled data sources (require an entry in `config.toml` to
  actually be polled; otherwise they are idle but invokable)
- `actions/` - on-demand operations triggered by the frontend
- `views/<name>/` - HTML views served at `quantum://plugin/<plugin>/views/<name>/`

Anything else in the folder (a README, helper assets, etc.) is ignored
by Quantum. Subfolders whose names start with `.` are also skipped.

## The simplest possible plugin: just a view

Drop `~/.config/quantum/plugins/hello/views/hello/index.html`:

```html
<!doctype html>
<html>
  <head><meta charset="utf-8" /></head>
  <body>
    <button>Hello from a plugin</button>
  </body>
</html>
```

Restart `quantumd`, then:

```bash
quantumctl call view.show '{"name":"plugin/hello/hello"}'
```

A WebKit window opens showing the button. That's it. No Rust, no config,
no scripts.

## Adding a polled provider

A polled provider runs a shell script on a schedule and publishes its
stdout as an event. Views subscribe to the channel and re-render when
the event fires.

`~/.config/quantum/plugins/moon-distance/scripts/moon-distance`:

```bash
#!/usr/bin/env bash
# Prints JSON: {"distance_km": <number>}
distance_km=$(curl -s 'https://api.example/moon' | jq -r '.km')
printf '{"distance_km": %s}\n' "$distance_km"
```

Mark it executable:

```bash
chmod +x ~/.config/quantum/plugins/moon-distance/scripts/moon-distance
```

Polled scripts are **idle by default**. They are never auto-scheduled
unless you opt them in via `config.toml`. Create the file:

`~/.config/quantum/plugins/moon-distance/config.toml`:

```toml
[scripts.moon-distance]
interval = 3600
# channel = "custom.event"  # optional override, defaults to "<plugin>.<script>"
```

The interval is in seconds. The minimum is 5 seconds; anything lower is
rejected at startup with a warning and the script is treated as idle.

## Adding an action

Actions are executables triggered explicitly by the frontend through
the IPC `action.invoke` call. They are always available; no config
needed.

`~/.config/quantum/plugins/moon-distance/actions/open-calendar`:

```bash
#!/usr/bin/env bash
exec xdg-open https://www.timeanddate.com/moon/phases/
```

Mark it executable too.

## Writing the view

`~/.config/quantum/plugins/moon-distance/views/moon-widget/index.html`:

```html
<!doctype html>
<html>
  <head>
    <meta charset="utf-8" />
    <link rel="stylesheet" href="style.css" />
  </head>
  <body>
    <button id="moon-btn">Loading...</button>
    <script src="script.js"></script>
  </body>
</html>
```

`style.css`:

```css
body {
  margin: 0;
  background: transparent;
  font-family: system-ui, sans-serif;
  color: #cdd6f4;
}
#moon-btn {
  background: #313244;
  color: #cdd6f4;
  border: 1px solid #45475a;
  border-radius: 6px;
  padding: 6px 12px;
  cursor: pointer;
  font: inherit;
}
```

`script.js`:

```javascript
// Quantum injects window.quantum.createClient() before this script
// runs. No imports, no bundler, no @quantum/client npm package.
const client = window.quantum.createClient();
const btn = document.getElementById('moon-btn');

function render(state) {
  if (!state) return;
  const thousands = Math.round(state.distance_km / 1000);
  btn.textContent = 'Moon: ' + thousands + ' thousand km';
}

// Default channel is "<plugin-name>.<script-name>".
client
  .call('provider.query', { id: 'moon-distance.moon-distance' })
  .then(render);

client.subscribe('moon-distance.moon-distance', render);

btn.addEventListener('click', () => {
  client.call('action.invoke', {
    provider: 'moon-distance',
    action: 'open-calendar',
  });
});
```

## Running it

Restart `quantumd` so it picks up the new plugin folder. The startup
log will include lines like:

```
Registered plugin 'moon-distance'
Loaded 1 plugins (1 polled scripts, 0 idle scripts, 1 actions, 1 views)
```

Then show the view:

```bash
quantumctl call view.show '{"name":"plugin/moon-distance/moon-widget"}'
```

Clicking the button invokes `open-calendar`, which opens your browser.
Every hour (the configured interval), the script re-runs and the button
updates with the new distance.

## The IPC surface in `window.quantum`

The injected client wraps the daemon's existing JSON-RPC bridge. Two
methods are exposed:

`window.quantum.createClient().call(method, params): Promise<unknown>`

  Makes an IPC call. The most useful methods for plugin authors:

  - `provider.query` with `{ id: "<channel>" }` - fetches the most
    recent payload published on a channel.
  - `action.invoke` with `{ provider: "<plugin-name>", action: "<name>" }` -
    runs an action script. Returns its stdout as the response.

`window.quantum.createClient().subscribe(channel, callback): () => void`

  Subscribes to event broadcasts on a channel. Returns an unsubscribe
  function. The callback receives the payload of each event published
  on that channel.

For the full IPC surface (action.schedule, view.show, etc.) see
`docs/protocol.md`.

## Channel naming

The default channel for a polled script is `<plugin-name>.<script-basename-without-extension>`.
For our moon-distance example: `moon-distance.moon-distance`.

You can override this in `config.toml`:

```toml
[scripts.moon-distance]
interval = 3600
channel = "moon_distance.event"
```

If two plugins claim the same channel name (only possible via explicit
overrides), the alphabetically-earlier plugin wins. The other one's
script is downgraded to idle and a warning is logged at startup.

## Environment variables for scripts

When Quantum runs a script in `scripts/` or `actions/`, it sets two
environment variables:

- `QUANTUM_PLUGIN_NAME` - the plugin's folder name
- `QUANTUM_PLUGIN_DIR` - absolute path to the plugin's folder

This lets a script reference its sibling files without hardcoding the
path:

```bash
cat "$QUANTUM_PLUGIN_DIR/data/template.json"
```

The working directory of the spawned process is also the plugin's
folder, so relative paths work too.

## Failure modes

- **Script exits non-zero**: a warning is logged with the exit code and
  stderr tail. No event is published. The next tick retries.
- **Script timeout** (interval - 1s, minimum 4s): the script is killed,
  a warning is logged, no event published.
- **Stdout above 1 MiB**: the output is truncated to 1 MiB with a
  warning. Useful events are usually well under this; large outputs
  suggest the script should aggregate or filter.
- **Stdout is empty**: no event published this tick.
- **Stdout is valid JSON**: published as a JSON event payload.
- **Stdout is non-JSON**: published as a string event payload (subscribers
  receive a plain string instead of a JSON object).

Identical successive payloads are dropped (the publish is
change-gated), so a script that always prints the same thing produces
exactly one event per state transition.

## Reloading after editing

`quantumctl call plugin.reload '{}'` re-walks the plugins directory
and prints how many plugins were discovered. **v1 limitation**: this
does NOT pick up newly-added plugins for polling or stop tasks for
plugins removed since startup. Use it to verify that Quantum *can see*
a new plugin folder before restarting the daemon. A full restart is
still required to actually start polling a new plugin's scripts or
stop polling a removed one.

A future release may add atomic registry swap and task cancellation.

## What plugins can NOT do (yet)

v1 ships only the shell-script primitive. If you want to subscribe to
a DBus signal, watch a file with inotify, or poll an HTTP API at sub-
second cadence, you currently need to wrap that work in a shell script
(use `gdbus monitor`, `inotifywait`, `curl` in a loop, etc.). The shell
overhead is real, but for the case where the operation is intrinsically
slow (HTTP fetches, periodic system summaries) the overhead is
acceptable.

Native primitives (`dbus.signal`, `http.poll`, `file.watch`, etc.) may
be added later as additional convention-based file kinds. The current
folder layout was chosen so a future release can introduce them
without breaking existing plugins.

## Sharing a plugin

A plugin folder is self-contained. You can:

- Commit it to a git repo and let users `git clone` into their plugins
  directory.
- Tar it up and ship it as a single archive.
- Document the `chmod +x` step in your README - executable bits do not
  always survive copying.

There is no central plugin registry. Distribution is whatever you make
of it.
