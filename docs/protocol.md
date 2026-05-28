# Protocol Reference

Quantum communicates via JSON-RPC 2.0 over Unix sockets. This document describes every available method.

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
    provider_id: string,            // which provider owns this action
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
    providers_count: number,
    themes_count: number
  }
}
```

**Example:**

```json
{"jsonrpc":"2.0","id":1,"result":{"version":"0.1.0","providers_count":4,"themes_count":1}}
```

---

## Error Codes

Standard JSON-RPC error codes apply, plus domain-specific codes:

| Code | Error | Meaning |
|------|-------|---------|
| -32600 | Invalid Request | Malformed JSON or missing required fields |
| -32601 | Method not found | Unknown method name |
| -32602 | Invalid params | Parameters don't match method signature |
| -32603 | Internal error | Serialization or other internal failure |
| -32001 | ProviderNotFound | Provider ID doesn't exist |
| -32002 | InvalidQuery | Query validation failed |
| -32003 | ActionFailed | Action execution failed |
| -32004 | Unsupported | Operation not supported |

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
  provider_id: 'apps',
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
