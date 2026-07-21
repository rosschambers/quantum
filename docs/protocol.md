# Protocol Reference

Quantum communicates via JSON-RPC 2.0 over Unix sockets. The
[Method Index](#method-index) lists every method the dispatcher accepts;
the sections that follow document a representative subset in full detail.
The authoritative method list lives in `Dispatcher::dispatch`
(`src/application/src/dispatcher.rs`).

## Connection

Connect to `$XDG_RUNTIME_DIR/quantum.sock` (or custom path via `quantumd --socket=<path>`).

Send requests as newline-delimited JSON:

```json
{"jsonrpc":"2.0","id":1,"method":"search","params":{...}}
{"jsonrpc":"2.0","id":2,"method":"system.status"}
```

Responses are also newline-delimited:

```json
{"jsonrpc":"2.0","id":1,"result":{...}}
{"jsonrpc":"2.0","id":2,"error":{"code":-32001,"message":"...","data":...}}
```

---

## Method Index

Every method routed by `Dispatcher::dispatch`
(`src/application/src/dispatcher.rs`). "Documented" marks methods with a full
request/response section below.

| Method               | Params (summary)                              | Result (summary)            | Documented |
|----------------------|-----------------------------------------------|-----------------------------|------------|
| `search`             | `{ text, providers?, limit? }`                | `{ matches, warnings }`     | Yes        |
| `action.invoke`      | `{ provider, action }`                         | `{}`                        | Yes        |
| `action.schedule`    | `{ delay_secs, label, action }`                | `{ id }`                    | Yes        |
| `action.cancel`      | `{ id }`                                        | `{}`                        | Yes        |
| `action.scheduled`   | none                                            | `{ jobs }`                  | Yes        |
| `provider.list`      | `{}`                                            | `string[]`                  | Yes        |
| `provider.subscribe` | `{ provider }`                                  | `{}`                        | Yes        |
| `provider.query`     | `{ id }`                                         | provider-specific JSON      | Yes        |
| `view.toggle`        | `{ name }`                                       | `{}`                        | Yes        |
| `view.show`          | `{ name }`                                       | `{}`                        | Yes        |
| `view.hide`          | `{ name }`                                       | `{}`                        | Yes        |
| `view.set_height`    | `{ name, height }`                               | `{}`                        | Yes        |
| `theme.reload`       | `{}`                                             | `{}`                        | Yes        |
| `theme.set`          | `{ theme }`                                       | `{}`                        | Yes        |
| `plugin.reload`      | none                                             | `{ loaded }`                | Yes        |
| `timer.create`       | `CreateTimerSpec`                                | `{ id }`                    | Yes        |
| `timer.list`         | none                                             | `TimerStoreData`            | Yes        |
| `timer.edit`         | `{ id, changes }`                                | `{}`                        | Yes        |
| `timer.cancel`       | `{ id }`                                          | `{}`                        | Yes        |
| `timer.dismiss`      | `{ id }`                                          | `{}`                        | Yes        |
| `timer.dismiss_all`  | none                                             | `{ dismissed }`             | Yes        |
| `system.status`      | `{}`                                             | `{ version, providers_count }` | Yes |
| `clipboard.clear`    | none                                             | `{}`                        | No         |

Any method not in this table returns `Unsupported` (`-32004`).

The `clipboard.clear` method empties the entire clipboard history, deleting every
stored entry and its on-disk blob. Clipboard history itself is read through the
`clipboard` provider (the `;` launcher prefix): searching lists recent entries,
selecting one re-copies it, and per-entry menu actions delete a single entry or
clear the whole history. Only the outright clear is exposed as a dedicated IPC
method.

---

## Methods

### search

Searches across registered providers for matches.

**Request:**

```typescript
{
  jsonrpc: "2.0",
  id: number,
  method: "search",
  params: {
    text: string,
    providers?: string[],           // provider IDs; empty = all providers
    limit?: number                  // max results; null = unlimited
  }
}
```

**Response:**

```typescript
{
  jsonrpc: "2.0",
  id: number,
  result: {
    matches: Match[],
    warnings: string[]              // timeouts, missing providers, etc.
  }
}
```

**Match object:**

```typescript
{
  id: string,
  provider: string,                 // provider ID that returned this match
  title: string,
  subtitle?: string,
  icon?: {
    kind: "name" | "path" | "data_uri",
    data: string
  },
  score: number,                    // 0.0–1.0 (higher is better)
  action: Action
}
```

**Action object:**

```typescript
// Launch a .desktop application
{
  kind: "launch",
  data: { desktop_id: string }
}

// Execute a shell command
{
  kind: "shell",
  data: {
    command: string[],
    terminal?: boolean              // default: false
  }
}

// Focus a window
{
  kind: "focus",
  data: { window_address: string }  // Hyprland window address hex
}

// Copy text to the clipboard (calc, emoji, and clipboard-history results)
{
  kind: "copy",
  data: { text: string }
}

// Custom action (plugin-defined)
{
  kind: string,
  data: serde_json::Value
}
```

**Example request:**

```bash
curl -X POST --unix-socket "$XDG_RUNTIME_DIR/quantum.sock" \
  -d '{"jsonrpc":"2.0","id":1,"method":"search","params":{"text":"firefox","providers":[]}}' | cat
```

---

### action.invoke

Invokes the action from a matched result.

**Request:**

```typescript
{
  jsonrpc: "2.0",
  id: number,
  method: "action.invoke",
  params: {
    provider: string,               // which provider owns this action
    action: Action
  }
}
```

**Response:**

```typescript
{
  jsonrpc: "2.0",
  id: number,
  result: {}                        // empty object on success
}
```

**Errors:**

- `-32003` (ActionFailed): Action could not be executed
- `-32001` (ProviderNotFound): Provider doesn't exist

---

### action.schedule

Schedule an `action.invoke` to fire after a delay. Jobs are held in memory
only (not persisted across daemon restarts). `delay_secs` must be in the
range `(0, 86400]` (24 hours).

**Request:**

```typescript
{
  jsonrpc: "2.0",
  id: number,
  method: "action.schedule",
  params: {
    delay_secs: number,             // seconds from now; (0, 86400]
    label: string,                  // human-readable label for the job
    action: {                       // same envelope as action.invoke params
      provider: string,
      action: Action
    }
  }
}
```

**Response:**

```typescript
{
  jsonrpc: "2.0",
  id: number,
  result: { id: string }            // 8-character hex job id
}
```

---

### action.cancel

Cancel a scheduled job by id before it fires.

**Request:**

```typescript
{
  jsonrpc: "2.0",
  id: number,
  method: "action.cancel",
  params: { id: string }            // job id from action.schedule
}
```

**Response:** `{ result: {} }`. Unknown ids return `Unsupported` (`-32004`).

---

### action.scheduled

List the currently scheduled jobs.

**Request:**

```typescript
{
  jsonrpc: "2.0",
  id: number,
  method: "action.scheduled",
  params: {}                        // no parameters
}
```

**Response:**

```typescript
{
  jsonrpc: "2.0",
  id: number,
  result: {
    jobs: Array<{
      id: string,
      label: string,
      fires_at: {                   // serde SystemTime shape
        secs_since_epoch: number,
        nanos_since_epoch: number
      }
    }>
  }
}
```

---

### provider.list

List all registered provider IDs.

**Request:**

```typescript
{
  jsonrpc: "2.0",
  id: number,
  method: "provider.list",
  params: {}                        // no parameters
}
```

**Response:**

```typescript
{
  jsonrpc: "2.0",
  id: number,
  result: string[]                  // array of provider IDs
}
```

**Example:**

```json
{"jsonrpc":"2.0","id":1,"result":["apps","shell","hyprland","pacman-updates"]}
```

---

### provider.subscribe

Start a provider's event stream so it begins publishing on
`<provider>.event` (see [AGENTS.md](../AGENTS.md) channel naming). Providers
that do not implement `subscribe` return `Unsupported` (`-32004`).

**Request:**

```typescript
{
  jsonrpc: "2.0",
  id: number,
  method: "provider.subscribe",
  params: { provider: string }      // provider ID to subscribe to
}
```

**Response:** `{ result: {} }`.

---

### provider.query

Run a provider's one-shot query and return its provider-specific JSON
payload verbatim.

**Request:**

```typescript
{
  jsonrpc: "2.0",
  id: number,
  method: "provider.query",
  params: { id: string }            // provider ID to query
}
```

**Response:** `{ result: <provider-specific JSON> }`.

---

### view.toggle

Toggle the visibility of a view (show if hidden, hide if shown).

**Request:**

```typescript
{
  jsonrpc: "2.0",
  id: number,
  method: "view.toggle",
  params: {
    name: string                    // view name (e.g., "launcher")
  }
}
```

**Response:**

```typescript
{
  jsonrpc: "2.0",
  id: number,
  result: {}
}
```

---

### view.show

Show a view window.

**Request:**

```typescript
{
  jsonrpc: "2.0",
  id: number,
  method: "view.show",
  params: {
    name: string
  }
}
```

**Response:**

```typescript
{
  jsonrpc: "2.0",
  id: number,
  result: {}
}
```

---

### view.hide

Hide a view window.

**Request:**

```typescript
{
  jsonrpc: "2.0",
  id: number,
  method: "view.hide",
  params: {
    name: string
  }
}
```

**Response:**

```typescript
{
  jsonrpc: "2.0",
  id: number,
  result: {}
}
```

---

### view.set_height

Set the content height of a view window (used by content-sized views that
report their measured height back to the host).

**Request:**

```typescript
{
  jsonrpc: "2.0",
  id: number,
  method: "view.set_height",
  params: {
    name: string,
    height: number                  // pixels (u32)
  }
}
```

**Response:** `{ result: {} }`.

---

### theme.reload

Reload the active theme from disk (hot reload).

**Request:**

```typescript
{
  jsonrpc: "2.0",
  id: number,
  method: "theme.reload",
  params: {}
}
```

**Response:**

```typescript
{
  jsonrpc: "2.0",
  id: number,
  result: {}
}
```

---

### theme.set

Switch to a named theme and reload.

**Request:**

```typescript
{
  jsonrpc: "2.0",
  id: number,
  method: "theme.set",
  params: { theme: string }         // theme name
}
```

**Response:** `{ result: {} }`.

---

### plugin.reload

Re-discover and reload plugins. Takes no parameters.

**Request:**

```typescript
{
  jsonrpc: "2.0",
  id: number,
  method: "plugin.reload",
  params: {}
}
```

**Response:**

```typescript
{
  jsonrpc: "2.0",
  id: number,
  result: { loaded: number }        // count of plugins loaded
}
```

---

### timer.create

Create a timer. Publishes the updated timer state on `timer.event`. The full
TypeScript shapes for `CreateTimerSpec`, `TimerStart`, `VisualConfig`, and
`NotifyConfig` are hand-written in `@quantum/client` (`src/ui/packages/client/src/timer.ts`).

**Request:**

```typescript
{
  jsonrpc: "2.0",
  id: number,
  method: "timer.create",
  params: {
    label: string,
    start:                          // tagged union on "kind"
      | { kind: "duration", secs: number }
      | { kind: "at", time: TimeOfDay }
      | { kind: "recurring", days: WeekdaySet, time: TimeOfDay },
    visual?: VisualConfig,          // defaults to subsystem config when omitted
    notify?: NotifyConfig
  }
}
```

**Response:**

```typescript
{
  jsonrpc: "2.0",
  id: number,
  result: { id: string }            // new timer id
}
```

---

### timer.list

List all timers and current timer settings. Takes no parameters.

**Request:**

```typescript
{
  jsonrpc: "2.0",
  id: number,
  method: "timer.list",
  params: {}
}
```

**Response:** `{ result: TimerStoreData }` where `TimerStoreData` is
`{ settings, timers }` (see `@quantum/client` `timer.ts`).

---

### timer.edit

Apply a partial update to an existing timer. Each supplied field replaces the
old value; omitted fields are unchanged. Supplying `time` (and `days` for
recurring timers) reschedules and re-arms the timer.

**Request:**

```typescript
{
  jsonrpc: "2.0",
  id: number,
  method: "timer.edit",
  params: {
    id: string,
    changes: {                      // all fields optional
      label?: string,
      visual?: VisualConfig,
      notify?: NotifyConfig,
      time?: TimeOfDay,
      days?: WeekdaySet,
      scatter_pos?: { x: number, y: number },
      duration_secs?: number
    }
  }
}
```

**Response:** `{ result: {} }`.

---

### timer.cancel

Remove a single timer by id.

**Request:**

```typescript
{
  jsonrpc: "2.0",
  id: number,
  method: "timer.cancel",
  params: { id: string }
}
```

**Response:** `{ result: {} }`.

---

### timer.dismiss

Dismiss a single timer by id (clears a fired timer from the widget).

**Request:**

```typescript
{
  jsonrpc: "2.0",
  id: number,
  method: "timer.dismiss",
  params: { id: string }
}
```

**Response:** `{ result: {} }`.

---

### timer.dismiss_all

Dismiss every timer. Takes no parameters.

**Request:**

```typescript
{
  jsonrpc: "2.0",
  id: number,
  method: "timer.dismiss_all",
  params: {}
}
```

**Response:**

```typescript
{
  jsonrpc: "2.0",
  id: number,
  result: { dismissed: number }     // count of timers dismissed
}
```

---

### system.status

Get daemon status and metadata.

**Request:**

```typescript
{
  jsonrpc: "2.0",
  id: number,
  method: "system.status",
  params: {}
}
```

**Response:**

```typescript
{
  jsonrpc: "2.0",
  id: number,
  result: {
    version: string,                // e.g., "0.1.0"
    providers_count: number
  }
}
```

Note: an earlier `themes_count` field was removed — the theme store exposes no
enumeration capability, so the handler no longer reports a fabricated count.

**Example:**

```json
{"jsonrpc":"2.0","id":1,"result":{"version":"0.1.0","providers_count":4}}
```

---

## Error Codes

Two error types reach the IPC wire: `quantum_domain::DomainError`
(`src/domain/src/error.rs`) and `quantum_application::ApplicationError`
(`src/application/src/error.rs`). Each exposes a stable `rpc_code()`.
Domain error codes occupy the range `-32000..-32099` and are part of the
public IPC contract: **stable; do not renumber**. New variants allocate the
next free code in the domain range.

There is no separate infrastructure error code range. Provider-layer
failures are `ProvidersError`, which carry no RPC code of their own — they
are converted into `DomainError` (typically `ActionFailed` or
`Unsupported`) before crossing IPC, so they surface with a domain code.

### Standard JSON-RPC codes

| Code   | Error            | Meaning                                          |
|--------|------------------|--------------------------------------------------|
| -32600 | Invalid Request  | Malformed JSON or missing required fields        |
| -32601 | Method not found | Unknown method name                              |
| -32602 | Invalid params   | Parameters don't match method signature          |
| -32603 | Internal error   | Untyped internal failure (`ApplicationError::Unknown`) |

### Domain errors (`-32000..-32099`)

Source: `quantum_domain::DomainError` (`src/domain/src/error.rs`).

| Code   | Variant            | Meaning                                  |
|--------|--------------------|------------------------------------------|
| -32001 | `ProviderNotFound` | Provider ID doesn't exist                |
| -32002 | `InvalidQuery`     | Query validation failed                  |
| -32003 | `ActionFailed`     | Action execution failed                  |
| -32004 | `Unsupported`      | Operation not supported / unknown method |

### Application errors

Source: `quantum_application::ApplicationError`
(`src/application/src/error.rs`). The `Domain` and `Dispatch` variants
delegate to `DomainError::rpc_code` above; only `Unknown` allocates a code,
and it reuses the standard JSON-RPC internal-error code.

| Code   | Variant            | Meaning                                       |
|--------|--------------------|-----------------------------------------------|
| (delegated) | `Domain`      | Wraps a `DomainError`; uses its domain code   |
| (delegated) | `Dispatch`    | Wraps a `DomainError` with the failing method |
| -32603 | `Unknown`          | Untyped internal failure                      |

Timer-subsystem errors (`TimerError`) are likewise mapped onto
`DomainError` before reaching IPC: `NotFound` becomes `Unsupported`
(`-32004`) and every other variant becomes `ActionFailed` (`-32003`).

---

## Client Examples

### quantumctl (CLI)

```bash
# Search
quantumctl search firefox

# List providers
quantumctl provider list

# Show launcher view
quantumctl show launcher

# Toggle launcher
quantumctl toggle launcher

# Reload theme
quantumctl theme reload

# Get status
quantumctl system status
```

### TypeScript / Browser

Use `@quantum/client`:

```typescript
import { createClient } from '@quantum/client'

const client = createClient()

// Search
const results = await client.call('search', {
  text: 'firefox',
  providers: [],
  limit: 10
})

// Invoke action
await client.call('action.invoke', {
  provider: 'apps',
  action: results.matches[0].action
})

// Toggle launcher
await client.call('view.toggle', { name: 'launcher' })
```

### Bash / netcat

```bash
SOCK="$XDG_RUNTIME_DIR/quantum.sock"

# Simple request
echo '{"jsonrpc":"2.0","id":1,"method":"system.status","params":{}}' | nc -U "$SOCK"

# With curl
curl -X POST --unix-socket "$SOCK" \
  -d '{"jsonrpc":"2.0","id":1,"method":"search","params":{"text":"fire"}}' \
  -H "Content-Type: application/json"
```

---

## See Also

- [AGENTS.md](../AGENTS.md) — Error handling philosophy
- [docs/architecture.md](architecture.md) — Data flow diagrams
