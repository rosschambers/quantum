# Quantum Repository Review — Verified Findings (2026-06-18)

## Method

Ten reviewer agents (two each for architecture, performance, security, build
time, and documentation) produced candidate findings. A separate verifier then
read every cited file and its surrounding context, confirmed each `file:line`
construct still exists and the reasoning holds, corrected line numbers that had
drifted, merged duplicates that recurred across reviewers and dimensions, and
discarded anything false, already-mitigated, or generic. Only independently
verified findings appear below. Each finding cites the line numbers actually
observed in the current tree, not the (sometimes drifted) numbers the reviewers
supplied. A final section lists candidates that were rejected, with reasons.

## Summary

| Dimension      | critical | high | medium | low | Total |
| -------------- | -------- | ---- | ------ | --- | ----- |
| Architecture   | 0        | 0    | 4      | 9   | 13    |
| Performance    | 0        | 1    | 4      | 8   | 13    |
| Security       | 0        | 1    | 3      | 5   | 9     |
| Build Time     | 0        | 1    | 3      | 5   | 9     |
| Documentation  | 0        | 6    | 2      | 4   | 12    |
| **Total**      | **0**    | **9**| **16** | **31** | **56** |

> **Remediation progress (updated 2026-06-18).** Six waves complete; the full
> workspace gate is green (fmt, clippy `-D warnings`, all Rust tests, every
> frontend test suite, `quantumd` build). Fixed: all 12 Documentation and all 9
> Build Time findings; the high-severity timers-rAF perf finding; the
> notification store cap + remote-icon rejection; the `desktop_apps`
> icon/scan/`Exec` fixes; IPC socket hardening + connection limit; several
> provider perf/security items; and the cheaper architecture items
> (`themes_count`, bar dependency, subscribe idempotency, domain-allowlist
> test). Per-finding status markers (✅ fixed, ◐ partial, ⏸ deferred) appear
> under each heading.
>
> **Still open / deferred** (with reasons in-line): the high-severity WebView
> bridge CSP/capability model (needs design + live GUI verification); MPRIS
> signal subscription, audio sink caching, per-window channel routing, shared
> `WebContext`, and the event-publish re-parse (perf, mostly needing a live
> session or a port change); Wi-Fi PSK via stdin (needs live Wi-Fi); and the
> larger architecture refactors (`DomainError` taxonomy, dispatcher registry,
> `timer_service` split, typed `Action::Custom`, channel-constant source of
> truth, `ProviderSource::snapshot`, legacy alias removal) — all low/medium and
> better done deliberately than in an automated pass.

Highest-severity verified findings (top of the fix queue):

1. WebView bridge exposes the full dispatcher (including `action.invoke` →
   `Action::Shell`) to page JavaScript with no Content-Security-Policy or
   navigation policy — any XSS, malicious theme, or malicious plugin page is
   local code execution. (security, high)
2. `shell.nix` regenerates a random `mktemp -d` pkg-config shim path on every
   shell entry, churning the GTK/WebKit `-sys` build-script fingerprints and
   forcing the documented full rebuild every `devsh.sh` invocation. (build, high)
3. The timers view runs an unconditional `requestAnimationFrame` loop on a
   per-monitor, auto-show, fill-output background surface — it animates at the
   monitor refresh rate on every output for the daemon's lifetime even with
   zero timers. (performance, high)
4. `docs/protocol.md` documents `quantum_infrastructure::InfrastructureError`
   and error codes `-32100..-32199` that exist nowhere in the source. (docs, high)
5. `docs/protocol.md` documents `action.invoke`'s parameter as `provider_id`;
   the handler deserializes `provider`. A client following the docs fails. (docs, high)
6. `docs/protocol.md` claims to document "every available method" but covers 8
   of the ~23 dispatcher methods. (docs, high)
7. `docs/architecture.md` module map is stale: one infrastructure crate (really
   seven), ~4 providers (really 18), and UI files that do not exist. (docs, high)
8. Rust version drift: docs and `rust-toolchain.toml` pin 1.85, the nix-shell
   actually ships 1.95, and `rust-toolchain.toml` is ignored by the nix build. (docs, high)

---

## Architecture

### `domain` depends on `futures`, violating the documented allowlist
- **Status:** ✅ Resolved (`e8a102f`) — blessed in the AGENTS.md allowlist with the `BoxStream`-only exception rather than refactored.
- **Severity:** medium
- **Verified locations:** `src/domain/Cargo.toml:13`;
  `src/domain/src/ports.rs:24`; allowlist in `AGENTS.md:18,27`;
  enforcement gap in `tests/architecture-test/src/lib.rs:170-181`.
- **Evidence:** `Cargo.toml` declares `futures = { version = "0.3", ... }`, and
  `ProviderSource::subscribe` returns
  `Option<futures::stream::BoxStream<'static, serde_json::Value>>`. `AGENTS.md`
  states domain may use "only `thiserror`, `serde`, `serde_json`,
  `async-trait`" and "must not gain non-serde dependencies" / import "any
  async/IO crate." `futures` is exactly such a crate. The architecture test only
  inspects intra-workspace edges (external crates are skipped at line 173-176),
  so the violation is unenforced.
- **Suggested improvement:** Either replace the `BoxStream` return with a
  domain-owned stream abstraction / `async-trait` channel, or formally amend the
  allowlist in `AGENTS.md` to include `futures`. Pick one; today the code and
  the contract disagree.
- **Raised by:** architecture (A), documentation (BK).

### Architecture test enforces only crate-edge direction, none of the documented source-level invariants
- **Status:** ◐ Partial (`7506bb5`) — added tests asserting the `domain` dependency allowlist (`thiserror`, `serde`, `serde_json`, `async-trait`, `futures`) and no `chrono`/`time` in domain. A "no `tokio::spawn` in application" check was intentionally not added (application legitimately uses it).
- **Severity:** medium
- **Verified locations:** `tests/architecture-test/src/lib.rs:100-145,170-181`.
- **Evidence:** `check_edge` only classifies crates by directory and checks
  layer-to-layer dependency direction. None of the documented invariants are
  tested: the domain external-dependency allowlist (see the `futures`
  finding), "no `chrono`/`time` in domain", and "no `tokio::spawn` in
  application" (violated in `subscribe_provider.rs:31` and `timer_service.rs`).
- **Suggested improvement:** Add manifest/source assertions: parse
  `src/domain/Cargo.toml` against an explicit allowlist, and grep application
  sources for forbidden symbols. The existing cargo-metadata harness is the
  natural home.
- **Raised by:** architecture (K).

### Unsupervised `tokio::spawn` in the application layer; non-idempotent `provider.subscribe` exposed over IPC
- **Status:** ◐ Partial (`07a3a50`) — `execute` is now idempotent per provider id (a `Mutex<HashSet<ProviderId>>` prevents duplicate forwarders). Supervising/retaining the spawned `JoinHandle` for shutdown remains deferred.
- **Severity:** medium
- **Verified locations:** `src/application/src/use_cases/subscribe_provider.rs:31-37`;
  IPC route `src/application/src/dispatcher.rs:82`; pre-subscribe loop
  `src/binaries/quantumd/src/main.rs:752-778`.
- **Evidence:** `SubscribeProviderUseCase::execute` calls `tokio::spawn` and
  discards the `JoinHandle`, so the forwarder is unsupervised. There is no
  dedup: each `provider.subscribe` call spawns a fresh forwarder on the same
  `{id}.event` channel. The method is reachable both at startup and over IPC, so
  a second subscribe duplicates every event for that provider.
- **Suggested improvement:** Track active subscriptions in the use case (a
  `Mutex<HashSet<ProviderId>>` or `HashMap<ProviderId, AbortHandle>`) and make
  `execute` idempotent; retain the handle for shutdown.
- **Raised by:** architecture (D).

### `DomainError` too coarse; typed error enums collapse to `(code, message)` on the wire
- **Status:** ◐ Partial (`b94f907`) — added a `DomainError::NotFound(String)` variant with stable code `-32005` and remapped `TimerError::NotFound` to it (no longer masquerading as `Unsupported`). Whether to send the full serde-tagged error body in `error.data` (vs the current code+message) remains a deferred decision.
- **Severity:** medium
- **Verified locations:** `src/domain/src/error.rs:7-16`; timer mapping
  `src/application/src/error.rs:51-59`; wire flattening
  `src/infrastructure/ipc/src/server.rs:25-29,230-235` and
  `src/binaries/quantumd/src/main.rs:56-58,66-67,103-104`.
- **Evidence:** `DomainError` has four variants and no generic not-found, so
  `TimerError::NotFound` is mapped onto `DomainError::Unsupported` (a semantic
  mismatch the code comments openly acknowledge). Although `ApplicationError`
  and `DomainError` derive serde tags (`#[serde(tag="kind", content="data")]`),
  the IPC boundary lowers every error to a `DispatchError { code, message }`
  via `application_error_parts`, so the tagged structure never crosses IPC and
  the derives are effectively dead for the wire format.
- **Suggested improvement:** Add a dedicated not-found variant (or a timer error
  range) and decide whether the serde-tagged error bodies should actually be
  sent in the JSON-RPC `error.data`; if not, drop the unused derives.
- **Raised by:** architecture (C).

### Dispatcher god-object
- **Severity:** low
- **Verified locations:** `src/application/src/dispatcher.rs:12-24` (11 `Arc`
  fields), `:45-72` (11-arg constructor with
  `#[allow(clippy::too_many_arguments)]`), `:74-102` (22-arm string match plus
  catch-all); constructor duplicated at `src/binaries/quantumd/src/main.rs:799-811`
  and in tests at `dispatcher.rs:498-510`.
- **Evidence:** Confirmed counts as cited (22 method arms, not 23). The
  constructor is hand-duplicated in production and test wiring.
- **Suggested improvement:** Group the use cases into a context struct, or
  register handlers in a map keyed by method name to collapse the match and the
  argument list.
- **Raised by:** architecture (G).

### `timer_service.rs` mixes orchestration, scheduling, persistence, and state-machine in one 1387-line file
- **Severity:** low
- **Verified location:** `src/application/src/use_cases/timer_service.rs` (1387
  lines, confirmed by `wc -l`).
- **Suggested improvement:** Split arming/scheduling, persistence orchestration,
  and the recurrence state machine into separate modules.
- **Raised by:** architecture (H).

### `system.status` returns hardcoded `themes_count: 1`
- **Status:** ✅ Fixed (`2a0eced`, `df4e603`) — the fabricated field was removed (the theme store has no enumeration capability); the protocol doc was updated to match.
- **Severity:** low
- **Verified location:** `src/application/src/dispatcher.rs:178`.
- **Evidence:** The handler emits `"themes_count": 1` literally; the value is
  never computed from the theme store. The same fiction appears in the
  `docs/protocol.md:330` example.
- **Suggested improvement:** Either query the theme store for a real count or
  drop the field; update the doc example to match.
- **Raised by:** architecture (I), documentation (BF example).

### Hand-mirrored Rust↔TypeScript DTOs with no drift guard, plus a dead `generated.ts` stub re-export
- **Status:** ◐ Partial (`1171093`) — dead `generated.ts` stub and its re-export removed. Relocating the `bar_state` DTOs into `@quantum/client` and adding a drift guard remain deferred (larger change).
- **Severity:** medium
- **Verified locations:** Rust source DTOs `src/domain/src/timer.rs`,
  `src/domain/src/bar_state.rs`; hand-written TS mirrors
  `src/ui/packages/client/src/timer.ts`, `src/ui/plugins/bar/views/bar/src/lib/types.ts`
  (re-types `SystemStats`, `MprisState`, `ActiveWindowState`,
  `MonitorActiveWindowState`, `PowerState`, `NetworkConnection`, etc.);
  dead stub `src/ui/packages/client/src/generated.ts` re-exported by
  `src/ui/packages/client/src/index.ts:111`.
- **Evidence:** The `bar_state` family is mirrored in a plugin-local
  `types.ts`, not in `@quantum/client`, breaking the AGENTS.md convention that
  all IPC DTOs live in `@quantum/client`. `generated.ts` contains only
  `export {};` with a comment claiming a build step will populate it, yet
  `index.ts` still does `export * from './generated'`.
- **Suggested improvement:** Move shared DTOs into `@quantum/client`, delete the
  dead `generated.ts` and its re-export, and add a CI check (even a snapshot
  test) that the hand-written types match the Rust shapes.
- **Raised by:** architecture (B), documentation (BB).

### Plugin views import `@quantum/client` without declaring it as a dependency
- **Status:** ✅ Fixed (`ee899d4`) — the bar view now declares `"@quantum/client": "workspace:*"`.
- **Severity:** low
- **Verified location:** `src/ui/plugins/bar/views/bar/package.json:1-20` (no
  `dependencies` block; `@quantum/client` imported in the view's
  `App.svelte`/`lib`).
- **Evidence:** The bar view relies on pnpm workspace hoisting; the timers and
  launcher views declare the dependency explicitly.
- **Suggested improvement:** Add `"@quantum/client": "workspace:*"` to the bar
  view's `dependencies`.
- **Raised by:** architecture (J).

### Legacy view-name alias table runs in parallel with the canonical scheme and is still load-bearing
- **Severity:** low
- **Verified location:** `src/ui/host/src/registry.rs:76-110` (`resolve_alias`,
  `resolve_alias_warning`), used in the open path at `:267-270`.
- **Evidence:** A static map translates legacy bare names to canonical
  `plugin/<plugin>/<view>` names, with a deprecation warning on every hit;
  config-override canonicalization depends on it.
- **Suggested improvement:** Schedule removal once configs/themes are migrated;
  until then keep it as the single source of legacy string literals (it already
  is).
- **Raised by:** architecture (P).

### Stringly-typed `Action::Custom { kind, payload: Value }` with ad-hoc per-provider JSON walking
- **Severity:** low
- **Verified locations:** `src/domain/src/action.rs:17-20`; consumers walk the
  payload by hand, e.g. `src/infrastructure/providers/src/notifications.rs:377-411`.
- **Suggested improvement:** Define typed per-provider command enums deserialized
  at the provider boundary instead of repeated `payload.get(...).and_then(...)`.
- **Raised by:** architecture (E).

### Channel / method / provider-id strings duplicated across Rust and TypeScript with no single source of truth
- **Severity:** low
- **Verified locations:** Rust `src/application/src/use_cases/subscribe_provider.rs:30`
  (`format!("{provider_id}.event")`); TS `src/ui/packages/client/src/timer.ts:125`
  (`const TIMER_CHANNEL = 'timer.event'`).
- **Suggested improvement:** Centralize channel/method constants on each side and
  cover the contract with a shared fixture test.
- **Raised by:** architecture (L).

### `ProviderSource::subscribe()` overloaded for both streaming and one-shot snapshot
- **Status:** ✅ Fixed (`26dd151`) — added an explicit `async fn snapshot()` to `ProviderSource` (default `None`); `QueryProviderUseCase` now prefers it and falls back to the subscribe-first-emission path. `notifications` overrides it with a payload byte-identical to its stream's first emission.
- **Severity:** low
- **Verified locations:** `src/domain/src/ports.rs:24`; one-shot consumer
  `src/application/src/use_cases/query_provider.rs:36-55` (2s timeout at `:27`).
- **Evidence:** `QueryProviderUseCase` takes the first emission of `subscribe()`
  within 2 seconds, relying on the unwritten invariant that "first emission is
  current state." That invariant holds today (e.g. the notifications stream
  yields an initial snapshot) but is enforced only by convention.
- **Suggested improvement:** Add an explicit `snapshot()` port method, or
  document the first-emission invariant on the trait.
- **Raised by:** architecture (M).

### WebView dispatch path re-parses JSON three times and maintains two parallel `Dispatcher` traits
- **Severity:** low
- **Verified locations:** bridge parses to `Value`
  `src/ui/host/src/bridge.rs:63-98`; UI `Dispatcher` trait takes `Value`
  `src/binaries/quantumd/src/main.rs:74-108`; the adapter re-encodes to
  `RawValue` (`to_raw_value`) at `:90` so the application dispatcher can parse
  again; two `DispatchError` types (`quantum_ipc` vs `quantum_ui::dispatcher`).
- **Evidence:** The ui crate cannot depend on infrastructure, so the
  `Value`→`RawValue`→parse round trip is a deliberate consequence of the layer
  rule, but it does defeat the `RawValue` zero-copy design on the WebView path.
- **Suggested improvement:** Define the `RawValue`-based dispatcher contract in a
  shared lower layer (domain/application) both ui and ipc can use.
- **Raised by:** architecture (F).

---

## Performance

### Timers view runs an unconditional `requestAnimationFrame` loop on a per-monitor fill-output surface
- **Status:** ✅ Fixed (`0ab0634`) — the `$effect` now depends on `timers.length` and early-returns (cancelling any pending frame) when the list is empty, so the loop runs only while timers exist.
- **Severity:** high
- **Verified locations:** `src/ui/plugins/timers/views/timers/src/App.svelte:46-55`;
  surface descriptor `src/ui/plugins/timers/views/timers/view.toml:2-6`
  (`per_monitor = true`, `auto_show = true`, `fill_output = true`).
- **Evidence:** The `$effect` schedules `requestAnimationFrame(loop)` with no
  dependency on `timers.length`, so it runs at the monitor refresh rate
  continuously. Because the view is per-monitor + auto-show, an instance exists
  on every output for the daemon's lifetime, animating even when no timers
  exist. The fill-output background surface also captures pointer input over the
  whole monitor (a known follow-up noted in AGENTS.md).
- **Suggested improvement:** Gate the rAF loop on `timers.length > 0` (start it
  when the first timer arrives, cancel when the list empties), or tick on a
  coarser interval when nothing is counting down.
- **Raised by:** performance (Q, both reviewers).

### `desktop_apps` resolves icons with synchronous disk walks on the search hot path, before truncation
- **Status:** ✅ Fixed (`3cca989`) — icons now resolved only for the post-truncation survivors (≤ `limit` per keystroke) through a per-name memo cache, so repeated keystrokes never re-walk the disk.
- **Severity:** medium
- **Verified locations:** `src/infrastructure/providers/src/desktop_apps.rs:64-71`
  (`resolve_icon_path` → `freedesktop_icons::lookup(name).with_size(48).find()`),
  called from `match_from_app` `:199-200`, invoked inside the search loop at
  `:306`, with `truncate(limit)` only afterward at `:320-322`.
- **Evidence:** For a non-empty query every app scoring above 0.1 gets a
  synchronous icon-theme disk lookup before the result set is truncated to the
  requested limit, on an async task, on every keystroke. (The empty-query path
  at `:248-254` resolves only after `take(limit)`, so it is bounded.)
- **Suggested improvement:** Resolve icons after sorting and truncating to the
  limit, cache resolved paths per icon name, and move the lookup onto
  `spawn_blocking`.
- **Raised by:** performance (S).

### MPRIS polls every player at 1 Hz with 3–4 D-Bus round trips each and no `PropertiesChanged` subscription
- **Severity:** medium
- **Verified locations:** `src/infrastructure/providers/src/mpris.rs:400-401`
  (1s interval), `:449-465` (tick branch calls `refresh_all_players`),
  `:295-312` (`refresh_all_players` loops players), `:209-258`
  (`fetch_player_state` issues `PlaybackStatus`, `Metadata`, and conditionally
  `Position` property gets). The comment at `:459-460` admits it refreshes
  "without an explicit PropertiesChanged subscription."
- **Evidence:** While any player is registered, the provider wakes every second
  and issues 3–4 D-Bus property reads per player. (It correctly skips when no
  players are present, `:455-456`.)
- **Suggested improvement:** Subscribe to `org.freedesktop.DBus.Properties`
  `PropertiesChanged` on each player and only poll `Position` while a player is
  actually `Playing`.
- **Raised by:** performance (V).

### Notification store re-serializes the entire store on every change and grows unbounded
- **Status:** ◐ Partial (`fe41d44`) — unbounded growth fixed with a `MAX_NOTIFICATIONS = 100` cap (oldest evicted), which also bounds the per-event snapshot cost. True per-event delta emission (vs full snapshot) remains deferred (needs a frontend protocol change).
- **Severity:** medium
- **Verified locations:** `src/infrastructure/providers/src/notifications.rs:152-173`
  (`snapshot_json` maps the whole `store`), invoked on every event in the
  subscribe stream `:442-447`; unbounded push at `:132-143` and `:228-239`;
  never-expire entries (`timeout_ms == 0`) via `resolve_timeout_ms:63-73`.
- **Evidence:** Each create/update/dismiss rebuilds and re-serializes every
  active notification into a fresh `Vec<Value>`. The store has no cap; `Notify`
  is callable by any session-bus client, and notifications that request
  never-expire (or critical urgency with server-default timeout) are never
  auto-evicted, so the store and per-event serialization cost both grow without
  bound. (This finding is the performance and security view of the same code;
  see the security section for the DoS framing.)
- **Suggested improvement:** Cap the store (ring-buffer / max count), evict on
  expiry server-side, and emit per-event deltas instead of a full snapshot each
  time.
- **Raised by:** performance (Y), security (AG).

### Audio provider re-runs `pactl get-default-sink` + full `pactl list sinks` on every relevant event
- **Severity:** low
- **Verified locations:** `src/infrastructure/providers/src/audio.rs:291-311`
  (`get_sink_info` runs `pactl list sinks`), `:334-345`
  (`current_audio_state_value` calls `get_default_sink` then `get_sink_info`),
  gated by `should_refresh_for_pactl_line:322-329`.
- **Evidence:** Each `sink`/`server` subscribe line spawns two `pactl`
  subprocesses, one of which dumps every sink in long form. The code already
  filters out high-frequency `sink-input`/`source` events, so this is bounded
  but still a full dump per volume change.
- **Suggested improvement:** Parse the target sink directly, or keep a cached
  sink table updated incrementally rather than re-dumping all sinks.
- **Raised by:** performance (Z).

### Every provider event is forwarded into every open WebView regardless of subscription
- **Severity:** low
- **Verified locations:** `src/ui/host/src/windows/widget.rs:392-465`;
  `src/ui/host/src/windows/panel.rs:232-305`.
- **Evidence:** Each window subscribes to the full broadcast channel and emits
  `window.__quantum_notify(channel, payload)` for every event; the JS client
  filters by channel, but the per-window `evaluate_javascript` cost (and the
  string formatting) is paid first. Events are coalesced per main-loop turn,
  which softens but does not remove the cost.
- **Suggested improvement:** Track per-window channel interest and skip
  forwarding events no view on that window subscribes to.
- **Raised by:** performance (U).

### One `WebView::new()` per window with no shared `WebContext`
- **Severity:** low
- **Verified locations:** `src/ui/host/src/windows/widget.rs:324`;
  `src/ui/host/src/windows/panel.rs:182`.
- **Evidence:** Each window constructs a standalone `webkit6::WebView::new()`
  with no shared `WebContext` or related-view, so WebKit spins up an independent
  network/web process per window; on N monitors the per-monitor bar multiplies
  that by N. (Exact process accounting is WebKit-internal, but no sharing is
  configured in the code.)
- **Suggested improvement:** Construct views with a shared `WebContext` /
  `WebView::with_related_view` so they pool a process and network session.
- **Raised by:** performance (W).

### `desktop_apps` startup scan uses blocking `std::fs` calls inside async functions
- **Status:** ✅ Fixed (`9886d6e`) — the startup scan now runs on `tokio::task::spawn_blocking`.
- **Severity:** low
- **Verified locations:** `src/infrastructure/providers/src/desktop_apps.rs:144`
  (`std::fs::read_dir`), `:149` (`std::fs::read_to_string`), inside the async
  `scan_directory`/`scan_apps`.
- **Evidence:** These block the async runtime, but only once at startup
  (`DesktopAppsProvider::new`), so the impact is bounded to cold start.
- **Suggested improvement:** Wrap the scan in `tokio::task::spawn_blocking` (the
  plugin walk already does this at `main.rs:213`).
- **Raised by:** performance (T).

### `proc_stats` wakes every second to read and parse `/proc` regardless of subscribers
- **Status:** ✅ Fixed (`e3c10f5`) — the loop now skips the `/proc` read/parse when `tx.receiver_count() == 0`.
- **Severity:** low
- **Verified location:** `src/infrastructure/providers/src/proc_stats.rs:28-53`.
- **Evidence:** The spawned loop sleeps 1s, then reads and parses `/proc/stat`
  and `/proc/meminfo` every tick for the process lifetime. A change-gate
  (`send_if_changed`, `:52`) suppresses redundant broadcasts but not the
  per-second read/parse, which runs even when no bar is subscribed or visible.
- **Suggested improvement:** Pause the loop when the broadcast channel has no
  receivers (`tx.receiver_count() == 0`).
- **Raised by:** performance (AB).

### Hyprland active-window serializes the entire multi-monitor state on every event
- **Status:** ✅ Fixed (`42364fe`) — state is now compared with `PartialEq` against the last broadcast first, and `serde_json::to_value` runs only when a broadcast will actually be sent.
- **Severity:** low
- **Verified location:** `src/infrastructure/providers/src/hyprland_active_window.rs:151-157`.
- **Evidence:** Every event (including frequent title changes) takes the state
  lock, applies the event, and `serde_json::to_value(&*guard)` over the whole
  multi-monitor struct before the change-gate (`send_state_if_changed`) decides
  whether to broadcast. Redundant broadcasts are suppressed, but title changes
  alter the payload and so are serialized and sent.
- **Suggested improvement:** Serialize lazily only when the gate will actually
  send, or diff at the struct level before serializing.
- **Raised by:** performance (AA).

### Event publish path serializes then re-parses each event
- **Severity:** low
- **Verified locations:** `src/application/src/use_cases/subscribe_provider.rs:33-35`
  (`payload.to_string()`); `src/binaries/quantumd/src/main.rs:131-139`
  (`RawValue::from_string(payload.to_string())`).
- **Evidence:** A provider `Value` is serialized to a string, passed as `&str`
  through the `EventBus` port, then re-validated/parsed back into a `RawValue`
  per event.
- **Suggested improvement:** Carry `Box<RawValue>` (or the original `Value`)
  through the port instead of stringifying and re-parsing.
- **Raised by:** performance (X).

### `BarClock` recomputes the full localized date string every second
- **Status:** ✅ Fixed (`49f09bc`) — `date` now derives from a day-granularity key, so `toLocaleDateString` runs once per calendar day while the time still updates every second.
- **Severity:** low
- **Verified location:** `src/ui/plugins/bar/views/bar/src/lib/BarClock.svelte:4-25`.
- **Evidence:** `now` updates every second; the `date` `$derived`
  (`toLocaleDateString`) recomputes each tick although the date is only used in
  the `title` tooltip and changes daily. Runs per monitor.
- **Suggested improvement:** Derive `date` from a day-granularity value, or
  recompute it only when the day changes.
- **Raised by:** performance (AC).

### WebView always enables console-to-stdout in production; empty-query default path does O(limit·n) lookups
- **Status:** ✅ Fixed (`3cca989`, `aa1ebfe`) — empty-query lookup is now an O(1) id index, and console-to-stdout is gated behind the `QUANTUM_INSPECTOR` flag in `widget.rs`/`panel.rs`.
- **Severity:** low
- **Verified locations:** `src/ui/host/src/windows/widget.rs:336` and
  `src/ui/host/src/windows/panel.rs:198`
  (`set_enable_write_console_messages_to_stdout(true)`, unconditional);
  `src/infrastructure/providers/src/desktop_apps.rs:248-254`
  (`ranked_ids.iter()...filter_map(|id| apps.iter().find(...))`).
- **Evidence:** Console piping is on regardless of `QUANTUM_INSPECTOR`. The
  empty-query default path linearly scans `apps` for each ranked id, i.e.
  O(limit·n).
- **Suggested improvement:** Gate console piping behind the inspector env flag;
  build an id→AppInfo index for the empty-query path.
- **Raised by:** performance (AD).

---

## Security

### WebView bridge exposes the full dispatcher to page JavaScript with no CSP or navigation policy
- **Status:** ⏸ Deferred — needs a design decision and live GUI verification. The effective fix is a capability model (untrusted view pages must not reach `action.invoke`/`Action::Shell`), but the launcher legitimately uses `action.invoke` to launch apps and the `>` shell provider, so a naive allowlist would break real features. A CSP + navigation policy is defence-in-depth but cannot be verified headlessly (a too-strict CSP could break every view's rendering). Recommend brainstorming this as its own task. The related console-to-stdout and socket-hardening pieces were completed in Wave 4a.
- **Severity:** high
- **Verified locations:** bridge forwards any `{method, params}` to the
  dispatcher `src/ui/host/src/bridge.rs:24-29,80-98`; WebView settings set no
  CSP/navigation policy and leave JavaScript enabled
  `src/ui/host/src/windows/widget.rs:334-338`,
  `src/ui/host/src/windows/panel.rs:196-202`; shell action path
  `src/infrastructure/providers/src/shell_command.rs:57-67` →
  `src/infrastructure/providers/src/shell.rs:92-105` (`spawn_detached`); shell
  provider registered at `src/binaries/quantumd/src/main.rs:490-498`.
- **Evidence:** Page JavaScript posts `{id, method, params}` over the `quantum`
  script-message channel and the bridge dispatches it verbatim — including
  `action.invoke` with `provider:"shell"` and `Action::Shell { command }`,
  which `spawn_detached` runs with no allowlist. No
  `connect_decide_policy`/navigation handler and no Content-Security-Policy are
  installed, so any XSS in a view, or any malicious theme or plugin page, gains
  the full dispatcher and thus arbitrary local command execution. (The
  same-user IPC socket is a weaker boundary because a same-user process can
  already run commands; the privilege escalation here is untrusted *web content*
  reaching the dispatcher.)
- **Suggested improvement:** Restrict the bridge to an allowlist of safe methods
  for view pages (search/view/theme/timer), require a stronger capability for
  `action.invoke`/`Action::Shell`, install a strict CSP and a navigation policy
  that blocks non-`quantum://` loads, and consider dropping `Action::Shell`
  reachability from web content entirely.
- **Raised by:** security (AF, AE).

### Unbounded notification store enables a memory-exhaustion DoS from any session-bus client
- **Status:** ✅ Fixed (`fe41d44`) — store capped at `MAX_NOTIFICATIONS = 100` with oldest-eviction on both the D-Bus and internal add paths. Per-sender `Notify` rate-limiting deferred (needs D-Bus sender identity); the cap bounds memory regardless of call rate.
- **Severity:** medium
- **Verified locations:** `src/infrastructure/providers/src/notifications.rs:94-149`
  (`apply_notify` pushes with no cap), `:228-239` (internal add), never-expire
  resolution `:63-73`; D-Bus `Notify` entry point `:505-530`.
- **Evidence:** Quantum is the `org.freedesktop.Notifications` server, so any
  app on the session bus can call `Notify` repeatedly. The store is an unbounded
  `Vec`; entries with `timeout_ms == 0` (and criticals requesting server
  default) never auto-evict, so a hostile or buggy client can grow memory
  without limit. (Performance impact of the per-event full re-serialize is
  tracked in the performance section.)
- **Suggested improvement:** Cap the store size, enforce server-side expiry for
  non-critical timeouts, and rate-limit `Notify`.
- **Raised by:** security (AG), performance (Y).

### D-Bus-supplied notification icon is loaded directly into a privileged `<img>` with no scheme validation or CSP
- **Status:** ✅ Fixed (`5ada7aa`) — `sanitize_icon` in `snapshot_json` now rejects remote URLs (`http(s)://` and any non-`quantum`/`file` scheme) server-side, so remote icon references never reach the webview. A CSP would add defence-in-depth (tracked under the WebView bridge finding).
- **Severity:** medium
- **Verified locations:** `src/ui/plugins/notification-center/views/toast/src/App.svelte:117`;
  `src/ui/plugins/notification-center/views/center/src/App.svelte:80`; icon
  carried verbatim through `notifications.rs:snapshot_json:162-166`.
- **Evidence:** The `app_icon` string from an arbitrary D-Bus `Notify` caller is
  bound to `<img src={notification.icon}>` with no scheme check, and (per the
  previous finding) there is no CSP. A notification can therefore force the
  privileged WebView to fetch an arbitrary remote URL — a tracking beacon /
  deanonymization vector. The shipped views otherwise use safe text
  interpolation (no `{@html}`), so this is the icon field specifically.
- **Suggested improvement:** Validate the icon is a freedesktop name or a
  local/`quantum://` path before rendering; reject `http(s)://`. A CSP that
  blocks remote image loads would also close this.
- **Raised by:** security (AH).

### Wi-Fi PSK passed as an `nmcli` argv element (visible in `/proc/<pid>/cmdline`)
- **Severity:** medium
- **Verified location:** `src/infrastructure/providers/src/wifi.rs:799-802`
  (`args.push("password"); args.push(pw);`), built in `connect_args:788-808`.
- **Evidence:** The pre-shared key is passed as a command-line argument to
  `nmcli`, so it is briefly readable in `/proc/<pid>/cmdline` by any process of
  the same user.
- **Suggested improvement:** Feed the secret via stdin or a NetworkManager
  connection profile (D-Bus `AddConnection`) rather than argv.
- **Raised by:** security (AI).

### `nmcli` invocation has no `--` end-of-options guard, allowing leading-dash SSID/BSSID option smuggling
- **Status:** ✅ Fixed (`8db99b8`) — `connect_args` now inserts `--` before the SSID. Unit-tested for argv shape; recommend one live `nmcli` connect to confirm the trailing `password`/`bssid` keywords still parse after `--`.
- **Severity:** low
- **Verified location:** `src/infrastructure/providers/src/wifi.rs:788-808`
  (`connect_args` builds `["device","wifi","connect", ssid, ...]` with no `--`).
- **Evidence:** A user-supplied SSID/BSSID starting with `-` is parsed by
  `nmcli` as an option rather than a value. This is argv-level option smuggling,
  not shell injection (the args are passed as a vector, not via `sh -c`).
- **Suggested improvement:** Insert `--` before the SSID, or validate that
  SSID/BSSID do not begin with `-`.
- **Raised by:** security (AJ).

### Spawned processes inherit the daemon's environment, stdio, and file descriptors
- **Status:** ◐ Partial (`5f99254`) — children now spawn in their own process group with stdio redirected to `/dev/null`. Environment minimization is deferred (launched GUI apps need the session env: `WAYLAND_DISPLAY`, `DBUS_SESSION_BUS_ADDRESS`, `PATH`).
- **Severity:** low
- **Verified location:** `src/infrastructure/providers/src/shell.rs:64-106`
  (`spawn_detached`: `Command::new` + args, then `spawn`).
- **Evidence:** No `setsid`/process-group isolation, no env scrubbing, and
  inherited stdio/fds. Launched apps and shell actions run in the daemon's
  process context.
- **Suggested improvement:** Set a new session/process group, null or controlled
  stdio, and a minimal environment for spawned children.
- **Raised by:** security (AK).

### IPC accept loop has no concurrent-connection limit and the socket is created with default permissions
- **Status:** ✅ Fixed (`7c2d91e`, `eebc1bd`) — socket chmod'd to 0600 after bind, `$HOME/.run` fallback created 0700, and the accept loop bounded by a 64-permit semaphore. e2e still passes. `SO_PEERCRED` peer-uid check deferred (0600 socket + 0700 dir already restrict to the owner).
- **Severity:** low
- **Verified locations:** `src/infrastructure/ipc/src/server.rs:77-97` (bind +
  unbounded `accept` loop spawning a task per connection); socket-path fallback
  `src/binaries/quantumd/src/main.rs:818-820` (`$HOME/.run` when
  `XDG_RUNTIME_DIR` is unset, directory not created 0700).
- **Evidence:** There is a per-line byte cap (`MAX_LINE_BYTES`, `:19`) but no cap
  on connection count, and no `SO_PEERCRED`/chmod/umask hardening on the socket.
  In normal operation `$XDG_RUNTIME_DIR` is mode 0700 so the socket is protected
  by its directory; the `$HOME/.run` fallback has no such guarantee and the
  directory is not created with restricted permissions.
- **Suggested improvement:** Bound concurrent connections (a semaphore), and
  create the fallback runtime directory 0700; optionally verify peer credentials.
- **Raised by:** security (AN, AE).

### `QUANTUM_SHELL_LOG` writes full command vectors to a file opened with default umask
- **Status:** ✅ Fixed (`5f99254`) — the log file is now created with mode 0600.
- **Severity:** low
- **Verified location:** `src/infrastructure/providers/src/shell.rs:70-90`.
- **Evidence:** When the env var is set, every command vector is appended to the
  named file via `OpenOptions::new().create(true).append(true)` with no explicit
  mode. This is an opt-in debug feature, so the exposure is conditional.
- **Suggested improvement:** Set restrictive file mode (0600) on creation.
- **Raised by:** security (AO).

### `.desktop` `Exec` parsing is naive whitespace-split and only strips `%` field codes
- **Status:** ✅ Fixed (`e7a83ba`) — replaced with a spec-aware `Exec` tokenizer honoring double-quote/backslash escaping and the full field-code set, used for both display and argv.
- **Severity:** low
- **Verified locations:** `src/infrastructure/providers/src/desktop_apps.rs:215-227`
  (`clean_exec`), argv built by `split_whitespace` at `:333-336`.
- **Evidence:** Quoting in `Exec=` is ignored, so a crafted desktop entry yields
  a wrong argv split. This is local (desktop files are user/system-installed),
  argv-based, not shell injection.
- **Suggested improvement:** Parse `Exec` per the freedesktop spec (handle
  quoting and field codes), or use a maintained parser.
- **Raised by:** security (AM).

---

## Build Time

> ✅ **All Build Time findings (AQ–AZ) fixed in Wave 1** — commits
> `7c2e230`, `7124f06`, `6530104`, `07565b3`, `e68ddef`, `e00b0bb`. Verified:
> `cargo build --workspace` + `clippy -D warnings` clean, `quantum-providers`
> tests pass, parallel frontend build green.

### `shell.nix` regenerates a random pkg-config shim path on every shell entry, forcing full rebuilds
- **Status:** ✅ Fixed (`7c2e230`)
- **Severity:** high
- **Verified location:** `shell.nix:34-38` (`PKG_CONFIG_SHIM="$(mktemp -d)"` then
  `export PKG_CONFIG_PATH="$PKG_CONFIG_SHIM:$PKG_CONFIG_PATH"`).
- **Evidence:** Every `nix-shell` entry (every `devsh.sh` invocation) creates a
  new random `mktemp -d` directory and prepends it to `PKG_CONFIG_PATH`. The
  GTK/WebKit/glib `-sys` build scripts capture `PKG_CONFIG_PATH` in their
  fingerprints, so the changing path invalidates them and triggers a from-scratch
  recompile of the dependency tree — exactly the "2–3 minute even when warm"
  churn documented in AGENTS.md.
- **Suggested improvement:** Use a stable shim location (e.g. a fixed path under
  the repo's `target/` or `$XDG_CACHE_HOME`, created idempotently) instead of
  `mktemp -d`, so `PKG_CONFIG_PATH` is identical across entries.
- **Raised by:** build (AQ, both reviewers).

### `quantumd` pins `glib = "0.19"` while the workspace's gtk4 0.9 uses glib 0.20, building a duplicate glib tree
- **Severity:** medium
- **Verified locations:** `src/binaries/quantumd/Cargo.toml:26-27` (`gtk4 = "0.9"`
  and `glib = "0.19"`); `Cargo.lock` contains both `glib 0.19.9` and
  `glib 0.20.12`.
- **Evidence:** gtk4 0.9 depends on the glib 0.20 family, but quantumd also
  declares glib 0.19 directly, so the lockfile carries two glib trees
  (`glib`/`glib-macros`/`glib-sys`/`gobject-sys`/`gio-sys` at 0.19 and 0.20),
  doubling that compile cost.
- **Suggested improvement:** Bump quantumd's `glib` to 0.20 to match gtk4 0.9
  (or drop the direct dependency and use gtk4's re-exported glib).
- **Raised by:** build (AR).

### `providers` pulls `zbus` with default features plus `tokio`, compiling the async-io backend alongside tokio
- **Severity:** medium
- **Verified locations:** `src/infrastructure/providers/Cargo.toml:30`
  (`zbus = { version = "4", default-features = true, features = ["tokio"] }`);
  contrast `src/infrastructure/dbus/Cargo.toml:16`
  (`default-features = false, features = ["tokio"]`).
- **Evidence:** With default features on, zbus's default `async-io` executor
  (and `async-executor`/`polling`) compiles even though the crate uses the tokio
  backend — the sibling `dbus` crate already does this correctly.
- **Suggested improvement:** Set `default-features = false` on the providers
  `zbus` dependency, matching `dbus`.
- **Raised by:** build (AS).

### No `[profile.*]` tuning in the root manifest
- **Severity:** medium
- **Verified location:** `Cargo.toml` (no `[profile.*]` section).
- **Evidence:** The dev profile therefore emits full DWARF debug info across the
  large gtk/webkit/glib tree, inflating compile and especially link time.
- **Suggested improvement:** Add a `[profile.dev]` with
  `debug = "line-tables-only"` and `split-debuginfo = "unpacked"`, and
  `[profile.dev.package."*"] debug = false` for dependencies.
- **Raised by:** build (AT).

### No faster linker or compilation cache configured
- **Status:** ✅ Fixed (`6530104`, then corrected) — `mold` is used for dev/CI builds, but configured via `shell.nix`'s shellHook (env `CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS`), NOT a committed `.cargo/config.toml`. The committed config forced mold on every cargo build of the repo and broke packaged/downstream nix builds (which lack mold) — it was removed. sccache not added.
- **Severity:** low
- **Verified locations:** no `.cargo/config.toml` (directory absent);
  `shell.nix:8-29` lists neither `mold`/`lld` nor `sccache`.
- **Evidence:** The webkit/gtk daemon links with the default linker and no
  compilation cache, leaving easy link-time wins on the table.
- **Suggested improvement:** Add `mold` (or `lld`) to the nix shell and a
  `.cargo/config.toml` `rustflags` to use it; optionally wire `sccache`.
- **Raised by:** build (AU).

### CI compiles the workspace multiple times
- **Status:** ◐ Partial — the `cargo test --workspace --no-run` attempt was reverted: it does not produce the `target/debug/quantumd` binary the e2e test spawns (caught by CI on a cold tree). Replaced with an explicit `cargo build -p quantumd` (leaner than the original `cargo build --workspace`, builds only the needed bin) before `cargo test --workspace`.
- **Severity:** low
- **Verified location:** `.github/workflows/ci.yml:83-89` (`cargo clippy
  --workspace --all-targets`, then `cargo build --workspace`, then `cargo test
  --workspace`).
- **Evidence:** Each step recompiles workspace crates; the standalone build
  exists only to materialize `target/debug/quantumd` for the e2e test.
- **Suggested improvement:** Rely on `cargo test --workspace` (which builds the
  binary the e2e test needs) and drop the separate `cargo build`, or use
  `--no-run`/artifact reuse to avoid redundant compiles.
- **Raised by:** build (AW).

### `build.rs` omits `rerun-if-changed` on the plugins/views parent directories
- **Severity:** low
- **Verified location:** `src/binaries/quantumd/build.rs:83-84` (emits
  `rerun-if-changed` only for each view's `dist/` and `view.toml`, never for
  the `plugins/` or `views/` parents).
- **Evidence:** A brand-new plugin or view directory is not watched until the
  build script has already seen it, which is the documented "second build needed
  to register a new view" behavior (acknowledged in the build.rs comment at
  `:70-74`).
- **Suggested improvement:** Also emit `rerun-if-changed` for the plugins-source
  and each `views/` directory so new entries are picked up on the first build.
- **Raised by:** build (AX).

### Frontend `pnpm -r build` runs the view builds without `--parallel`
- **Status:** ⏸ Reverted (`7fdfcca`→fix) — `--parallel` was tried but is incorrect here: the view packages import `@quantum/client` and need its `dist/` built first, so a non-topological parallel build races and fails to resolve the package (caught by CI on a cold tree). Reverted to the topological `pnpm -r build`. The original finding's premise (the builds were "serial") is fine — topological ordering is required, not a defect.
- **Severity:** low
- **Verified locations:** `justfile:30` (`pnpm -C src/ui -r build`);
  `.github/workflows/ci.yml:74` (`pnpm -r build`).
- **Evidence:** The recursive build runs in topological order rather than
  parallel across the independent view packages.
- **Suggested improvement:** Add `--parallel` (the views are independent) to the
  build target as the dev/test targets effectively allow.
- **Raised by:** build (AV).

### Workspace-root `tokio` enables `features = ["full"]`, consumed unscoped by thin crates
- **Severity:** low
- **Verified location:** `Cargo.toml:31` (`tokio = { version = "1", features =
  ["full"] }`).
- **Evidence:** Thin crates that take `tokio.workspace = true` (e.g. quantumctl,
  config) pull the full feature set; some crates already narrow features
  locally.
- **Suggested improvement:** Make the workspace default minimal and let each
  crate opt into the tokio features it needs.
- **Raised by:** build (AY).

### CI `nix_path` uses a moving channel rather than a pinned revision
- **Severity:** low
- **Verified location:** `.github/workflows/ci.yml:29`
  (`nix_path: nixpkgs=channel:nixos-unstable`).
- **Evidence:** A moving channel means the toolchain (and thus build-script
  fingerprints) can shift between runs, re-fingerprinting cached cargo
  artifacts.
- **Suggested improvement:** Pin a specific nixpkgs revision.
- **Raised by:** build (AZ).

---

## Documentation

> ✅ **All Documentation findings (BA–BO) fixed in Wave 1** — commits
> `f434f12`, `1171093`, `04f0c82`, `e2709d3`, `019fd2d`, `e8a102f`.

### Rust version drift: docs pin 1.85, nix ships 1.95, and `rust-toolchain.toml` is ignored by the nix build
- **Status:** ✅ Fixed (`f434f12`)
- **Severity:** high
- **Verified locations:** `AGENTS.md:82` ("Rust 1.85");
  `scripts/devsh.sh:3-4` ("Rust 1.85"); `rust-toolchain.toml:2` (`channel =
  "1.85"`); `shell.nix:10-11` installs nixpkgs `rustc`/`cargo` (no rustup);
  `.github/workflows/ci.yml:25` comment states the unstable channel provides
  "rustc 1.95".
- **Evidence:** The nix shell installs nixpkgs Rust (1.95 on the unstable
  channel) and never consults `rust-toolchain.toml` (no rustup in the
  environment), so the pinned 1.85 is fiction. The CI file itself documents the
  1.95 reality.
- **Suggested improvement:** Decide on one version. Either pin nixpkgs to a Rust
  1.85 release and keep the docs, or update `rust-toolchain.toml`, `AGENTS.md`,
  and `devsh.sh` to state the real version and note that nix, not rustup,
  selects it.
- **Raised by:** documentation (BA).

### Three contradictory TypeScript-codegen stories; none exist
- **Severity:** high
- **Verified locations:** `README.md:54` (`just ts-bindgen`, "Regenerate
  TypeScript types") — absent from `justfile`; `justfile:24` ("until specta is
  wired"); `src/ui/packages/client/src/generated.ts:1-4` ("populated by
  `quantum-dev generate-ts`") — no such subcommand; `AGENTS.md:71-76` correctly
  states there is no codegen.
- **Evidence:** Four documents disagree about whether and how TypeScript types
  are generated. The justfile target the README advertises does not exist, the
  stub file references a nonexistent dev subcommand, and the justfile comment
  invokes a third tool (specta).
- **Suggested improvement:** Keep AGENTS.md's accurate statement, delete the
  `just ts-bindgen` row from the README, remove `generated.ts` and its
  re-export, and drop the specta comment (or implement one path and document
  only that).
- **Raised by:** documentation (BB), architecture (B).

### `docs/protocol.md` claims to document every method but covers 8 of ~23
- **Severity:** high
- **Verified locations:** `docs/protocol.md:3` ("describes every available
  method"); documented set = search, action.invoke, provider.list, view.toggle,
  view.show, view.hide, theme.reload, system.status; full dispatcher
  `src/application/src/dispatcher.rs:74-100` (22 methods + catch-all).
- **Evidence:** Missing from the doc: `action.schedule/cancel/scheduled`,
  `provider.subscribe/query`, `view.set_height`, `theme.set`, `plugin.reload`,
  and the entire `timer.*` family (`create/list/edit/cancel/dismiss/dismiss_all`).
- **Suggested improvement:** Document the remaining methods, or soften the
  "every available method" claim and link to the dispatcher as source of truth.
- **Raised by:** documentation (BC).

### `docs/protocol.md` references an `InfrastructureError` type and error codes that do not exist
- **Severity:** high
- **Verified locations:** `docs/protocol.md:368-381` (`quantum_infrastructure::
  InfrastructureError`, codes `-32100..-32199`); real errors
  `src/domain/src/error.rs:25-33` (`-32001..-32004`),
  `src/application/src/error.rs:25-31` (`-32603`); provider errors are
  `ProvidersError` with no RPC code.
- **Evidence:** There is no `quantum_infrastructure` crate (infrastructure is
  seven separate crates) and no `InfrastructureError` with `-32100`-range codes;
  nothing in the source emits those codes. The documented table is invented.
- **Suggested improvement:** Delete the infrastructure-error section or replace
  it with the actual error model (`DomainError` codes plus the `-32603`
  internal-error fallback).
- **Raised by:** documentation (BD).

### `docs/protocol.md` documents `action.invoke` parameter as `provider_id`, but the handler reads `provider`
- **Severity:** high
- **Verified locations:** `docs/protocol.md:128` and TS example `:426`
  (`provider_id`); handler `src/application/src/dispatcher.rs:112-119`
  (`struct ActionInvokeParams { provider: String, action: Action }`).
- **Evidence:** A client following the documented field name sends `provider_id`,
  which fails to deserialize into `provider`.
- **Suggested improvement:** Fix the doc (and TS example) to `provider`, or
  add a serde alias accepting both.
- **Raised by:** documentation (BE).

### `docs/architecture.md` module map is stale
- **Severity:** high
- **Verified locations:** `docs/architecture.md:92-140`; real layout:
  infrastructure is seven crates (`config`, `dbus`, `hyprland`, `ipc`,
  `plugins`, `providers`, `theme-store`), `src/infrastructure/providers/src`
  has 18 providers, UI files are `panel.rs`/`widget.rs`/`multiplex.rs`/
  `registry.rs`/`window_host.rs`, and the domain file is `match_result.rs`.
- **Evidence:** The doc shows infrastructure as a single crate with submodules,
  lists only four providers (desktop_apps, shell_command, hyprland_windows,
  declarative_shell), references `ui/host/app.rs (QuantumApp)` and
  `windows/launcher.rs (LauncherWindow)` that do not exist, and names the domain
  file `match.rs` (actual `match_result.rs`).
- **Suggested improvement:** Regenerate the module map from the current tree;
  this section is misleading enough to send contributors to nonexistent files.
- **Raised by:** documentation (BF).

### README installation leads with an apt-get + `cargo install` path that bypasses the required nix shim
- **Severity:** medium
- **Verified locations:** `README.md:24-41` (apt-get and `cargo install
  --path`), `:64` ("build inside the container").
- **Evidence:** The advertised `apt-get install ...` + `cargo install` flow
  bypasses `shell.nix`, whose pkg-config shim aliases `gtk4-layer-shell.pc`; the
  Rust `gtk4-layer-shell = "0.5"` crate looks for `gtk4-layer-shell` while the
  system installs `gtk4-layer-shell-0.pc`, so the build fails without the shim.
  The "container" language is also stale (everything is nix-shell now).
- **Suggested improvement:** Lead with the nix-shell / `devsh.sh` flow, and mark
  the apt path as unsupported or document the required pkg-config alias.
- **Raised by:** documentation (BG).

### Packaged systemd unit `ExecStart` contradicts the documented nix-installed service model
- **Severity:** medium
- **Verified locations:** `packaging/systemd/quantum.service:6`
  (`ExecStart=%h/.cargo/bin/quantumd`); AGENTS.md:104-105 describes the running
  service as "the nix-installed binary."
- **Evidence:** The shipped unit runs `~/.cargo/bin/quantumd` (a `cargo install`
  path), while AGENTS.md and the README's nix-centric guidance describe a
  nix-installed daemon. The two packaging stories disagree.
- **Suggested improvement:** Pick one install story and align the unit file,
  README systemd section, and AGENTS.md.
- **Raised by:** documentation (BO).

### AGENTS.md `file:line` citations have drifted
- **Severity:** low
- **Verified locations:** AGENTS.md:99 cites `main.rs:782` (real socket guard
  `src/binaries/quantumd/src/main.rs:818-833`); AGENTS.md:154-156 cites
  `widget.rs:94` and `:198` (real `init_layer_shell()` at
  `src/ui/host/src/windows/widget.rs:102` and `:240`); AGENTS.md:156 cites
  `panel.rs:61` (real `should_use_layer_shell` at
  `src/ui/host/src/windows/panel.rs:46`).
- **Evidence:** The cited constructs exist but at the corrected lines above.
- **Suggested improvement:** Refresh the line numbers, or cite by function name
  instead of line.
- **Raised by:** documentation (BH).

### AGENTS.md cites a theme-store path that does not exist
- **Severity:** low
- **Verified location:** AGENTS.md:183 cites
  `src/infrastructure/src/theme/store.rs` (real path
  `src/infrastructure/theme-store/src/store.rs`).
- **Suggested improvement:** Correct the path.
- **Raised by:** documentation (BI).

### AGENTS.md onion table models infrastructure as a single crate
- **Severity:** low
- **Verified locations:** AGENTS.md:16-24 (single `src/infrastructure` row); real
  membership `Cargo.toml:6-12` (seven infrastructure crates); the architecture
  test deliberately permits sibling-infrastructure edges
  (`tests/architecture-test/src/lib.rs:119-130`).
- **Evidence:** The table's single-crate model understates the seven-crate
  reality and the sibling-dependency allowance the test encodes.
- **Suggested improvement:** Expand the table to list the infrastructure crates
  and note the sibling-edge rule.
- **Raised by:** documentation (BJ).

### Stale "container"/"devcontainer" terminology
- **Severity:** low
- **Verified locations:** `justfile:1-2`; `scripts/devsh.sh:3`; `README.md:64`.
- **Evidence:** All three describe a "dev container" / "docker-based
  devcontainer" though the environment is now nix-shell (`devsh.sh` is a
  nix-shell wrapper).
- **Suggested improvement:** Replace "container" wording with "nix-shell."
- **Raised by:** documentation (BL).

---

## Rejected / unverifiable candidates

- **R — `observedTotals` Map grows unbounded
  (`TimerVisual.svelte:26-33`).** Rejected. The `Map` is declared in the
  instance `<script>`, so it is per-component, not page-global. Each
  `TimerVisual` is created per timer inside a keyed `{#each ... (timer.id)}`
  block and only ever calls `trackTotal` with its own timer's id, so each
  instance's map holds a single entry; when the timer is dismissed the keyed
  block (and its map) is destroyed. There is no unbounded growth. The author's
  "lives for the page's lifetime" comment is misleading but the memory is
  reclaimed on component teardown.
- **AP — `notification-view-explorer.html` uses `innerHTML`
  (`src/ui/plugins/bar/views/bar/views/notification-view-explorer.html`).**
  Rejected as a shipped attack surface. The file is a standalone static-data
  demo page; `build.rs` only embeds `plugins/<plugin>/views/<view>/dist/index.html`,
  and this file is not under any `dist/`, so it is not compiled into quantumd
  and never served over `quantum://`. It is dead developer scaffolding (worth
  deleting for hygiene) but not a runtime vector. The shipped toast/center views
  use safe text interpolation.
- **BM — Per-crate `lib.rs` module docs are one-line stubs.** Dropped as generic
  "add more docs" boilerplate with no correctness impact; not a defect.
- **BN — `docs/plans/*` cited as current truth without a status caveat.**
  Dropped as a stylistic/boilerplate suggestion rather than a verifiable defect;
  the plans are clearly dated and the reviewer provided no concrete
  contradiction.
- **Security "sound" claims (not findings, confirmed correct):** the
  `quantum://` scheme handler rejects `./`, `../`, and absolute traversal
  (`src/ui/host/src/scheme.rs`) with a re-check in the theme store
  (`src/infrastructure/theme-store/src/store.rs`); external commands
  (`nmcli`/`hyprctl`/`pactl`) are invoked via argv vectors, not `sh -c`; and
  shipped Svelte views use text interpolation, not `{@html}`. These were
  reported as sound and verified as such — no action needed.
