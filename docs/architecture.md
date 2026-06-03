# Architecture

## Onion Layers

Quantum follows a strict onion architecture enforced at the Cargo crate level. Each layer has clearly defined dependencies and responsibilities.

### Layers (inner to outer)

#### Domain Layer (`src/domain`)

Pure business logic with no I/O, no async runtime, no external frameworks. Contains:

- **Value objects:** `ProviderId`, `WindowId`, `MatchScore`
- **Entities:** `Query`, `Match`, `Action`, `IconRef`
- **Errors:** `DomainError` with stable IPC codes
- **Ports (traits):** `ProviderSource`, `ProviderRegistry`, `ConfigStore`, `ThemeStore`, `ShellExecutor`, `HyprlandClient`, `WindowHost`, `EventBus`

**Dependencies:** Only `serde`, `serde_json`, `thiserror`, `async-trait`

#### Application Layer (`src/application`)

Use cases and business logic orchestration. Contains:

- **Use cases:** `SearchUseCase`, `LaunchActionUseCase`, `ListProvidersUseCase`, `ReloadThemeUseCase`, `OpenViewUseCase`
- **Dispatcher:** Routes JSON-RPC method calls to use cases
- **Errors:** `ApplicationError` wrapping domain errors

**Dependencies:** `domain` (path)

#### Infrastructure Layer (`src/infrastructure`)

Concrete implementations of domain ports and external integrations. Contains:

- **Providers:** `DesktopAppsProvider`, `ShellCommandProvider`, `HyprlandWindowsProvider`, `DeclarativeShellProvider`
- **Shell execution:** `TokioShellExecutor`
- **Configuration:** `ConfigStore` (TOML parser)
- **Themes:** `ThemeStore` (cascade resolution, hot reload)
- **IPC server:** JSON-RPC over Unix sockets
- **Hyprland client:** Command/event streaming

**Dependencies:** `domain` (path)

#### UI Layer (`src/ui/host`)

GTK4 and WebKitGTK windows, Svelte 5 frontends, and browser bridges. Contains:

- **App shell:** `QuantumApp` (GTK4 application)
- **Windows:** `LauncherWindow` (anchored via gtk4-layer-shell)
- **Bridge:** WebKit script message handler for bidirectional RPC
- **URI scheme:** `quantum://` handler for theme bundles
- **Window host:** `GtkWindowHost` (manages window lifecycle)

**Dependencies:** `application` (path)

#### Binaries

- **`quantumd`** — Daemon entrypoint. Initializes infrastructure, application, UI; runs Tokio + GTK main loop
  - Dependencies: `ui`, `application`, `infrastructure`, `domain`
- **`quantumctl`** — CLI client for IPC operations
  - Dependencies: `domain`, `infrastructure`
- **`quantum-dev`** — Development tools (TS codegen, theme watch)
  - Dependencies: `domain`, `infrastructure`

#### Testing Crate

- **`tests/architecture-test`** — Enforces dependency rules via `cargo_metadata`

---

## Dependency Rules

Every layer can only import from layers below it (and the workspace root).

```
domain ← application ← ui
   ↑          ↑        ↑
   └─ infrastructure ┘
           ↑
      (binaries)
```

**Forbidden patterns:**
- Domain importing any other workspace crate
- Application importing infrastructure or UI
- Infrastructure importing application or UI
- UI importing infrastructure directly (must route through application)

This constraint is enforced by an automated test: `cargo test -p quantum-architecture-test`

---

## Module Organization

```
src/
├── domain/
│   ├── ids.rs (ProviderId, WindowId)
│   ├── score.rs (MatchScore)
│   ├── query.rs (Query)
│   ├── match.rs (Match, IconRef)
│   ├── action.rs (Action enum)
│   ├── error.rs (DomainError with codes)
│   └── ports.rs (trait definitions)
├── application/
│   ├── error.rs (ApplicationError)
│   ├── dispatcher.rs (method routing)
│   └── use_cases/
│       ├── search.rs
│       ├── launch_action.rs
│       ├── list_providers.rs
│       ├── reload_theme.rs
│       └── open_view.rs
├── infrastructure/
│   ├── error.rs (InfrastructureError)
│   ├── registry.rs (InMemoryProviderRegistry)
│   ├── shell.rs (TokioShellExecutor)
│   ├── config/ (ConfigStore, TOML loader)
│   ├── theme/ (ThemeStore, cascade, manifest)
│   ├── ipc/ (JSON-RPC server)
│   ├── hyprland/ (client, events)
│   └── providers/
│       ├── desktop_apps.rs
│       ├── shell_command.rs
│       ├── hyprland_windows.rs
│       └── declarative_shell.rs
├── ui/
│   ├── host/
│   │   ├── app.rs (QuantumApp)
│   │   ├── bridge.rs (WebKit ↔ Dispatcher)
│   │   ├── scheme.rs (quantum:// handler)
│   │   ├── window_host.rs (GtkWindowHost)
│   │   └── windows/
│   │       └── launcher.rs (LauncherWindow)
│   ├── themes/ (Svelte 5 theme bundles)
│   └── packages/ (shared TS packages like @quantum/client)
└── binaries/
    ├── quantumd/ (daemon main)
    ├── quantumctl/ (CLI client)
    └── quantum-dev/ (dev tools)
```

---

## Data Flow

### Search Flow

```
quantumctl search "firefox"
    ↓
UnixSocket (JSON-RPC)
    ↓
Dispatcher::dispatch("search", params)
    ↓
SearchUseCase::execute(Query)
    ↓
foreach Provider::search(query) [timeout 2s per provider]
    ↓
aggregate, sort by score, truncate by limit
    ↓
SearchResponse { matches, warnings }
    ↓
JsonRpcResponse
    ↓
Unix socket → client
```

### Launch Flow

```
quantumctl action.invoke <action>
    ↓
Dispatcher::dispatch("action.invoke", params)
    ↓
LaunchActionUseCase::execute(provider_id, action)
    ↓
provider.invoke(action)
    ↓
DesktopAppsProvider → TokioShellExecutor::spawn_detached
    ↓
child process (firefox, etc.)
```

### Theme Hot-Reload

```
File watcher detects change
    ↓
ThemeStore::reload()
    ↓
EventBus::publish(ThemeReloaded)
    ↓
WebKit bridge posts to JS
    ↓
Frontend re-renders with new tokens/views
```

---

## Threading Model

Quantum runs two concurrent execution contexts:

### Main Thread — GTK Event Loop

GTK4 is single-threaded by design. The main OS thread runs `gtk4::Application::run()`, which:
- Owns all GTK4 window objects and WebKitGTK WebView instances
- Dispatches GTK signals in strict serial order
- Must never block (or the UI freezes)

### Worker Thread — Tokio Multi-Threaded Runtime

A background worker thread (`std::thread::spawn`) parks a `tokio::runtime::Runtime` that:
- Runs the IPC server and accepts client connections
- Drives provider searches (with per-provider 2-second timeouts)
- Executes shell commands via `TokioShellExecutor`
- Watches theme files and publishes events

### Cross-Thread Communication

```
┌─────────────────────────────┐
│   Tokio Worker Thread       │
│  (providers, IPC, events)   │
└──────────┬──────────────────┘
           │
      tokio::sync::mpsc
      UnboundedChannel
           │
    WindowRequest::Open
           │
┌──────────▼──────────────────┐
│   GTK Main Thread           │
│  (windows, WebKit, events)  │
└──────────┬──────────────────┘
           │
    glib::MainContext::spawn_local
    (back to Tokio on idle)
           │
┌──────────▼──────────────────┐
│   WebView Script Context    │
│  (JavaScript, DOM)          │
└─────────────────────────────┘
```

#### 1. Window Requests (Tokio → GTK)

When `OpenViewUseCase::execute` calls `WindowHost::open("launcher", mode)`:

- `GtkWindowHost::open` is called on a Tokio task
- It sends `WindowRequest::Open { view, mode }` on an `UnboundedChannel`
- The GTK thread drains this channel via `glib::MainContext::spawn_local` (async closure)
- `WindowRegistry::handle(req)` processes on the GTK main thread
- Actual window show/hide/toggle happens synchronously in GTK context

**Why this design:** GTK object construction and visibility toggling must happen on the main thread. Tokio tasks can't call GTK APIs directly.

#### 2. IPC Dispatcher Calls (WebView → Tokio)

When the launcher's TypeScript calls `window.webkit.messageHandlers.quantum.postMessage(...)`:

- The WebKit script message handler (`bridge.rs::register_bridge`) receives on the GTK main thread
- It clones the dispatcher and WebView, then calls `runtime.spawn(async move { ... })`
- The dispatcher runs on a Tokio worker task, calling into use cases
- When the result arrives, `glib::MainContext::default().spawn(...)` schedules a callback on GTK
- The callback calls `webview.evaluate_javascript(...)` to return the result to JS

**Why this design:** Providers and IPC logic are async and CPU-intensive. They run on Tokio. But WebView is `!Send`, so callbacks must execute back on GTK to call its methods.

#### 3. Theme Reload Events (File Watcher → WebView)

When the theme file watcher detects a change:

- `ThemeStore` publishes `Event::ThemeReloaded` on an `Arc<EventBus>`
- A broadcaster adapter converts this to an IPC notification
- The IPC server sends `{ "jsonrpc": "2.0", "method": "theme.reloaded", "params": {...} }` to all clients
- The WebView client's `bridge.ts` receives the notification and calls `window.__quantum_notify`
- The launcher's `App.svelte` subscription updates the `<style id="quantum-tokens">` innerHTML
- CSS repaints instantly (no page reload)

**Why this design:** Events are rare enough that a broadcast channel (not a dedicated sync primitive) is appropriate. Notifications traverse the same IPC wire as RPC calls, so the transport layer handles delivery.

### Send/Sync Guarantees

- **`DomainError`, `Query`, `Match`, `Action`:** All `Send + Sync + 'static`. Cloned freely across threads.
- **`Dispatcher`:** Held in `Arc<dyn IpcDispatcher>`. All implementations are `Send + Sync`.
- **`WindowHost`:** Held in `Arc<dyn WindowHost>`. `GtkWindowHost` is `Send` (tokio::sync::mpsc is `Send`).
- **`WebView`:** Is `!Send`. Held exclusively on GTK main thread. Never cloned across threads.
- **`gtk4::Window`:** Is `!Send`. Only accessed on GTK main thread.

### Synchronization Points

| Synchronization | Type | Purpose |
|---|---|---|
| `tokio::sync::mpsc::UnboundedChannel<WindowRequest>` | SPMC | Tokio spawns window requests; GTK drains |
| `glib::MainContext::spawn_local(...)` | Callback queue | Bridge returns results to WebView |
| `Arc<Dispatcher>` | Reference count | Share provider registry across threads |
| `Arc<ThemeStore>` | Reference count | Share theme data and file watcher |
| `EventBus` broadcast channel | Pub/Sub | Theme reload events to IPC subscribers |
| `tokio::runtime::Handle` | Handle clone | Pass async runtime into GTK closures |

---

## Key Design Decisions

1. **Onion architecture at the crate level** — Not just convention, but enforced by CI tests
2. **Error codes are stable and domain-owned** — Allows reliable RPC error handling (-32001 for ProviderNotFound, etc.)
3. **All IPC errors serializable** — `ApplicationError` and `DomainError` impl `Serialize`
4. **Per-provider 2-second timeout** — Slow providers don't block the entire search
5. **Theme cascade with max depth 8** — Prevents accidental cycles
6. **GTK on main, Tokio on worker** — Async I/O and providers run on Tokio; windows and WebKit on GTK
7. **`quantum://` URI scheme** — Theme bundles embedded via `include_dir!` or served from filesystem
8. **Hot reload without restart** — Token changes detected by file watcher, broadcast via IPC, injected into DOM

---

## See Also

- [AGENTS.md](../AGENTS.md) — Complete dependency table and rules
- [docs/protocol.md](protocol.md) — IPC method reference
- [docs/theming.md](theming.md) — Theme authoring guide
