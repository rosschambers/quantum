# Architecture

## Onion Layers

Quantum follows a strict onion architecture enforced at the Cargo crate level. Each layer has clearly defined dependencies and responsibilities.

### Layers (inner to outer)

#### Domain Layer (`crates/domain`)

Pure business logic with no I/O, no async runtime, no external frameworks. Contains:

- **Value objects:** `ProviderId`, `WindowId`, `MatchScore`
- **Entities:** `Query`, `Match`, `Action`, `IconRef`
- **Errors:** `DomainError` with stable IPC codes
- **Ports (traits):** `ProviderSource`, `ProviderRegistry`, `ConfigStore`, `ThemeStore`, `ShellExecutor`, `HyprlandClient`, `WindowHost`, `EventBus`

**Dependencies:** Only `serde`, `serde_json`, `thiserror`, `async-trait`

#### Application Layer (`crates/application`)

Use cases and business logic orchestration. Contains:

- **Use cases:** `SearchUseCase`, `LaunchActionUseCase`, `ListProvidersUseCase`, `ReloadThemeUseCase`, `OpenViewUseCase`
- **Dispatcher:** Routes JSON-RPC method calls to use cases
- **Errors:** `ApplicationError` wrapping domain errors

**Dependencies:** `domain` (path)

#### Infrastructure Layer (`crates/infrastructure`)

Concrete implementations of domain ports and external integrations. Contains:

- **Providers:** `DesktopAppsProvider`, `ShellCommandProvider`, `HyprlandWindowsProvider`, `DeclarativeShellProvider`
- **Shell execution:** `TokioShellExecutor`
- **Configuration:** `ConfigStore` (TOML parser)
- **Themes:** `ThemeStore` (cascade resolution, hot reload)
- **IPC server:** JSON-RPC over Unix sockets
- **Hyprland client:** Command/event streaming

**Dependencies:** `domain` (path)

#### UI Layer (`crates/ui`)

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

- **`crates/architecture-test`** — Enforces dependency rules via `cargo_metadata`

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
crates/
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
│   ├── app.rs (QuantumApp)
│   ├── bridge.rs (WebKit ↔ Dispatcher)
│   ├── scheme.rs (quantum:// handler)
│   ├── window_host.rs (GtkWindowHost)
│   └── windows/
│       └── launcher.rs (LauncherWindow)
└── bin/
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

## Key Design Decisions

1. **Onion architecture at the crate level** — Not just convention, but enforced by CI tests
2. **Error codes are stable and domain-owned** — Allows reliable RPC error handling (-32001 for ProviderNotFound, etc.)
3. **All IPC errors serializable** — `ApplicationError` and `DomainError` impl `Serialize`
4. **Per-provider 2-second timeout** — Slow providers don't block the entire search
5. **Theme cascade with max depth 8** — Prevents accidental cycles
6. **GTK and Tokio coexist** — GTK main loop receives commands via `glib::MainContext::spawn_local`
7. **`quantum://` URI scheme** — Theme bundles embedded via `include_dir!` or served from filesystem

---

## See Also

- [AGENTS.md](../AGENTS.md) — Complete dependency table and rules
- [docs/protocol.md](protocol.md) — IPC method reference
- [docs/theming.md](theming.md) — Theme authoring guide
