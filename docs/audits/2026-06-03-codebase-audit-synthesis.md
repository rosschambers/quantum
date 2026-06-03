# Quantum codebase audit — consolidated findings

8 read-only audits across domain, application, ipc+dbus, providers, theme+config+hyprland+plugins, ui/host, frontend bundles, and tooling.

## Total findings: ~140
## Real correctness bugs: 8
## High-impact efficiency wins: 12
## Simplifications worth doing: 30+
## Nits and "future work": rest

---

## TIER 1 — actual bugs (fix these first)

| # | Finding | File:line | Effort |
|---|---|---|---|
| B1 | `tokio::process::Command::spawn()` leaks zombie processes — every desktop app launch, every shell action | shell.rs:99-108, system_power/mod.rs:122-128 | S |
| B2 | Hyprland event stream has no reconnect; hyprland restart silently kills all hyprland-derived state forever | client.rs:104-122 | M |
| B3 | One bad interval in a plugin's config.toml drops the ENTIRE plugin (incl. unrelated scripts + views) | walker.rs:38-42 | S |
| B4 | IPC `read_line` is unbounded; any client can OOM the daemon by sending a stream with no newline | server.rs:141-143 | S |
| B5 | IPC broadcast-forwarder task leaks per disconnected client (held writer, held receiver, lag-blocks live consumers) | server.rs:103-132 | M |
| B6 | DBus `_interface` parameter silently ignored — every NM property change rebuilds full NetworkState | dbus/common.rs:78-83 | S |
| B7 | DBus providers stall forever if their service was down at startup (no NameOwnerChanged watcher) | dbus/common.rs:88-180 (5 providers affected) | M |
| B8 | Bluetooth adapter pick is nondeterministic (`HashMap` iteration); multi-adapter hosts see state flap | bluetooth.rs:336-350 | S |

## TIER 1.5 — claimed-but-broken (silent failures)

| # | Finding | File:line | Effort |
|---|---|---|---|
| B9 | `plugin_script` advertises `streamable: true` but `subscribe()` returns `None` (default) | plugin_script.rs:79-83 | S |
| B10 | CI uses Rust 1.82; workspace pins 1.85 — CI cannot build the project at all | .github/workflows/ci.yml:24-27 | trivial |
| B11 | `quantum-dev generate-ts` writes stale stubs to a file (generated.ts) — runs the wrong command will silently overwrite real content | quantum-dev/src/main.rs:48-100 | S |
| B12 | `ActiveWindow.svelte` works by luck — channel name was `'hyprland.activewindow.event'` (string) without using the constant; vulnerable to provider id refactor | bar/src/lib/ActiveWindow.svelte:21 + AGENTS.md convention | S |

---

## TIER 2 — high-impact perf wins (battery + responsiveness)

| # | Finding | Impact | Effort |
|---|---|---|---|
| P1 | DBus PropertiesChanged: signal payload discarded, helper does N round-trips per change. Affects battery, NM, BlueZ, PowerProfiles. NM is the worst — 7 round-trips per Connectivity flip | each provider does 5-7× more DBus traffic than needed | M |
| P2 | `audio.rs` spawns 4 pactl subprocesses per sink event. Volume notch = up to 4 forks. `pactl --format=json list sinks` collapses to 2 | high during media playback | S |
| P3 | `hyprctl j/clients` shells out per keystroke in the launcher; cache exists but is invalidated on every read | every launcher keystroke; 1+ subprocess+parse | M |
| P4 | `WebView.evaluate_javascript` is called once per broadcast event, per WebView (no batching) | ~20-40 JS evaluations/sec idle on multi-monitor | S |
| P5 | `resolved_tokens()` re-reads + re-parses tokens.toml on every HTML response on the GTK main thread | blocks UI thread; multiple HTML loads at startup | S |
| P6 | Provider boot is serial (`.await` per `connect`) — DBus discovery for 6 services in sequence | adds seconds to cold start | S |
| P7 | `SystemMeters.svelte` re-runs `smoothPath` (O(N) Catmull-Rom) twice per tick because `$derived` isn't used | 1Hz overhead, GPU+CPU | S |
| P8 | `BuildFn<S>` heap-allocates a fresh `BoxFuture` per DBus event | every NM/UPower signal | M |
| P9 | `desktop_apps.rs` calls `to_lowercase()` per app per keystroke | 200 apps × 5 lowercase ops/keystroke | S |
| P10 | `JsonRpcRequest::params: Value` instead of `RawValue` causes double-walk of every IPC payload | every IPC call | S |
| P11 | `BufWriter` missing on IPC sockets — 2 syscalls per response/event | ipc throughput | S |
| P12 | Brightness uses `std::fs::read_to_string` inside async fn | tiny but principled | trivial |

---

## TIER 3 — high-value simplifications

### Dead code to delete
| Finding | Files |
|---|---|
| `ConfigStore` trait: zero implementors, shadow-named with concrete struct | domain/ports.rs:45-49 |
| `EventBus::subscribe`: every impl is `Ok(())` no-op; never called | domain/ports.rs:75-78 |
| `ShellExecutor::execute`: dead trait method | domain/ports.rs:91 |
| `ProviderCapabilities`: every provider implements; no consumer reads it | domain/ports.rs:7-22 |
| `WindowId`: exported but unused | domain/ids.rs:31 |
| `socket.ts`: throws "not yet implemented" | client/src/socket.ts:6 |
| `tests/e2e/src/lib.rs`: empty crate that drags assert_cmd etc. into every workspace test build | tests/e2e/ |
| `unsafe impl Send for FakeFoo`: half-dozen unnecessary unsafe impls in dispatcher tests | application/src/dispatcher.rs:262-413 |

### Boilerplate to collapse
| Finding | Files |
|---|---|
| Dispatcher: 14 nearly-identical handlers + 3 copies of `ViewParams` struct (~80 lines removable) | application/src/dispatcher.rs:71-242 |
| quantumd: 13 mechanical `match XxxProvider::connect()` blocks (~190 lines) — single helper + `tokio::join!` | quantumd/src/main.rs:296-485 |
| Each crate re-implements `From<io::Error>` and `From<toml::de::Error>` (4 copies) | */src/error.rs |
| `BroadcastingEventBus` has two `IpcDispatcher` impls with identical bodies | quantumd/src/main.rs:47-72 |

### Default impls hiding bugs
| Finding | Files |
|---|---|
| `WindowHost::set_view_height` default `Ok(())` lets forgotten overrides silently clip popovers | domain/ports.rs:128-130 |
| `ThemeStore::get_plugin_file` default `None` lets forgotten overrides silently return 404 | domain/ports.rs:65-67 |

### Architecture-test brittleness
| Finding | Files |
|---|---|
| Hardcoded crate-name allow-list; new infra crate would inherit zero constraints | tests/architecture-test/src/lib.rs |

---

## TIER 4 — nits and future work

Suppressing for brevity — full lists in the 8 individual audit reports. Includes 30+ items like:
- `inject_tokens` does `String::replace` even when no placeholder exists
- `monitor_name` called twice per construct
- `power-menu/App.svelte` cleanup couples disarm timeout with subscription teardown
- pnpm-workspace.yaml's depth-2 wildcard limits future view nesting
- many more

---

## What to do with this

8 fixes in TIER 1 are real bugs — they should ship before anything in TIER 3 or TIER 4. The TIER 2 perf items are the answer to "did my battery improvements really land?" — most landed, but this audit found 12 more places to look.

The TIER 3 simplifications could be one focused session of `cargo +clippy -W deprecated-impl --warn` plus deletion. ~500 lines removed.
