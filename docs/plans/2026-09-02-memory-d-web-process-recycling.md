# Memory Option D: In-process recycling of the shared web process

Status: proposed, **deferred** — only build this if data after A + B + C shows the shared web
process still growing past acceptable bounds between daily recycles. Implementation lands in
this repo.

## Background

See `2026-09-02-memory-a-gsk-renderer-and-memoryhigh.md` for the incident and
`2026-09-02-memory-b-webkit-memory-pressure.md` for the shared-process architecture. Short
version: all warm views (bar × monitors, timers × monitors, launcher, clock, toast) share one
WebKitWebProcess via the hidden anchor WebView (`src/ui/host/src/web_process.rs:27-47`), whose
heap grows for the daemon's lifetime (135 MB → 526 MB cgroup anon over 13 h). Option B caps it
with WebKit's own pressure handler; option C resets it daily. D is the surgical version: quantum
itself recycles the shared render process at idle without restarting the daemon.

## Why deferred

- Highest engineering cost of the four options, touching the trickiest lifecycle code
  (`registry.rs` window management, layer-shell surface teardown — which already has
  documented abort pitfalls, see `panel.rs:425-431` comments).
- Options B (kill threshold + reload-on-terminate) and C (daily recycle) together deliver most
  of the benefit: B bounds the process, C bounds everything.
- D cannot address the Vulkan/GTK daemon-side growth at all — that is A's and C's territory.

## Design sketch

### Trigger

Recycle when ALL of:
1. Shared web process RSS exceeds a threshold (for example 350 MB), sampled every few minutes
   (read `/proc/<pid>/status` of the web process; quantum can find the pid via
   `WebView::web_process_identifier` or by walking its cgroup).
2. Session is idle: no visible transient panels, launcher hidden, no pointer over any quantum
   surface for N minutes (quantum already tracks input regions and view visibility in
   `WindowRegistry`).

### Mechanism

The shared process exists because every warm WebView was created `related_view(&anchor)`.
WebKit offers no "restart this render process in place" API, so recycling means rebuilding the
views:

1. Create a **new** anchor WebView with no relation to the old one (new render process).
2. For each warm window, in place: create a replacement WebView related to the new anchor, load
   the same `quantum://` URL and arguments, swap it into the GTK window's child slot when its
   `load-changed` reaches `Finished`, then unparent and drop the old WebView.
3. When the last old-related WebView drops, terminate the old anchor's web process explicitly
   (same rationale as `panel.rs:436-438` — GTK destroy alone leaves the process alive).
4. Views re-request their state through the existing plugin channels on load, same as after a
   daemon restart today.

Swap-on-load-finished (double-buffering the WebView) avoids the visible blank-bar flash that a
destroy-then-recreate sequence would produce; memory briefly doubles during the swap (two render
processes), which is fine at idle.

### State

Same constraint as option C: any warm view holding in-memory-only JS state loses it. The
recycle reuses the same view URLs and init-args path as a cold start, so anything that survives
a daemon restart survives this.

## Acceptance criteria

- [ ] Recycle triggers only at idle and only past the RSS threshold.
- [ ] All warm views render correctly after recycle on both monitors; no blank-bar flash longer
      than one frame; no layer-shell abort (the known `gdk_surface_get_display` teardown trap).
- [ ] Old web process is confirmed gone (pid absent) within 10 s of recycle completion.
- [ ] Shared-process RSS returns to fresh baseline (~50-80 MB) after recycle.
- [ ] Soak test: 20 consecutive forced recycles with no crash, no window leak
      (`hyprctl layers` count stable), no fd leak in quantumd.
- [ ] Transient panels opened mid-recycle behave correctly (either blocked until done or
      unaffected — decide and test).

## Risks

- Layer-shell surface lifecycle is the most fragile code in the host (documented aborts on
  hide-then-destroy ordering). Double-buffered swap adds a second live surface per window
  during the transition — needs careful sequencing on Hyprland.
- `related_view` chains: every view related to the old anchor must be replaced in the same
  cycle, or stragglers keep the old process alive.
- Complexity vs payoff: revisit the data first. If B's kill-threshold + reload path proves
  clean in practice, D may be entirely unnecessary — B's kill is the same reset, just less
  graceful.

## Related

- `2026-09-02-memory-a-gsk-renderer-and-memoryhigh.md`
- `2026-09-02-memory-b-webkit-memory-pressure.md`
- `2026-09-02-memory-c-scheduled-recycle.md`
