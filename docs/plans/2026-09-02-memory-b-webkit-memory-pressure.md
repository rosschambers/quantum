# Memory Option B: WebKit memory pressure settings and cache model

Status: proposed. Implementation lands in this repo (`src/ui/host/` + `src/binaries/quantumd/`).

## Background

See `2026-09-02-memory-a-gsk-renderer-and-memoryhigh.md` for the full incident. The second
growth vector (after the Vulkan state pools) is anonymous heap: cgroup `anon` grew from ~135 MB
fresh to 526 MB over 13 hours, with the single long-lived shared WebKitWebProcess at 430 MB RSS.

Quantum's architecture concentrates this deliberately: all warm views (bar × monitors,
timers × monitors, launcher, clock, toast) share **one** render process via the hidden anchor
WebView (`src/ui/host/src/web_process.rs:27-47`), so their DOM/JS heaps accumulate in one
process for the daemon's lifetime. Transient `destroy_on_dismiss` views already get isolated
processes that are explicitly terminated on close (`src/ui/host/src/windows/panel.rs:420-440`) —
that path is fine. The problem is the warm shared process, which currently runs with **zero**
memory configuration: no pressure handler tuning, no cache model, no limits
(`apply_widget_settings` at `src/ui/host/src/web_process.rs:60-75` disables features only).

## Design

Two changes, both on the shared `WebContext`.

### 1. `MemoryPressureSettings` on the WebContext

WebKitGTK exposes `WebKitMemoryPressureSettings` (webkit6 rust binding:
`webkit6::MemoryPressureSettings`). It is a **construct-only** property of `WebContext`, so the
code must stop using `WebContext::default()` (currently implicit via
`src/binaries/quantumd/src/gtk_loop.rs:95-98`) and construct one explicitly, then thread it to
everything that today grabs the default context.

Proposed starting values (tune with data):

| Setting | Value | Effect |
|---|---|---|
| `set_memory_limit` | 500 MB | Working-set target per web process |
| `set_conservative_threshold` | 0.5 | Begin releasing caches at 50 % of limit |
| `set_strict_threshold` | 0.8 | Aggressive release (drop JIT caches, GC hard) at 80 % |
| `set_kill_threshold` | 2.0 | Kill the web process at 2 × limit (1 GB) — last resort |
| `set_poll_interval` | 30 s | Check cadence |

Kill-threshold note: if the shared process is killed, every warm view dies with it. WebKit fires
`web-process-terminated` on each WebView; quantum must handle it by reloading the view (the
`quantum://` scheme makes reload cheap and stateless for widgets). Verify this handler exists or
add it — without it a kill produces the known "stranded empty bar" failure mode.

### 2. Cache model

`WebContext::set_cache_model(CacheModel::DocumentViewer)`. Quantum serves tiny local Svelte
bundles over `quantum://`; the default web-browser cache model keeps page/back-forward caches
sized for general browsing that quantum never benefits from.

## Implementation sketch

1. `web_process.rs`: build the shared `WebContext` once (lazy static or passed-in), constructed
   with `MemoryPressureSettings`; register the `quantum://` scheme on it (moves from
   `gtk_loop.rs`).
2. All `new_webview` paths use that context explicitly.
3. Add `web-process-terminated` handling: log at WARN with the reason, reload the WebView.
4. Make the five numbers configurable via the existing config system (or environment variables)
   so tuning does not require rebuilds.
5. Extend `scripts/measure-memory.sh` to also print cgroup `anon`/`shmem` so option A and B
   effects are separable.

## Acceptance criteria

- [ ] Shared web process RSS stays under 500 MB across a 48 h session with normal usage.
- [ ] `web-process-terminated` on a warm view reloads it automatically — no stranded blank bar
      (test by sending SIGKILL to the shared WebKitWebProcess).
- [ ] Transient panel behaviour unchanged (isolated process still terminated on dismiss).
- [ ] No functional regressions: launcher search, bar widgets, timers, toasts all work.
- [ ] Settings are tunable without a rebuild.

## Risks

- Construct-only context threading touches every view-creation path; moderate refactor risk in
  `registry.rs` / `panel.rs` / `widget.rs`.
- Aggressive strict thresholds can cause visible jank (cache refill, JIT re-warm) on animated
  widgets. Start conservative; tune with the 30 s poll data.
- The `related_view` process-sharing mechanism must keep working with an explicit context —
  verify the anchor WebView and all sharers use the same context instance.

## Related

- Option A must land first (or its A/B data collected) so anon-heap savings here are measurable
  in isolation from the Vulkan shmem noise.
