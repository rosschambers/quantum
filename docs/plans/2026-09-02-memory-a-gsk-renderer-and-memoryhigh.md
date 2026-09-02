# Memory Option A: Force the GL renderer and remove the MemoryHigh throttle trap

Status: proposed. Implementation lands in the config repo
(`code/infra/config/modules/features/home/quantum.nix`), not this repo.

## Background — the 2026-09-02 incident

quantumd froze after 13 hours: every thread stuck in uninterruptible sleep (`D` state) inside
`mem_cgroup_handle_over_high`. Because quantum's full-screen layer surfaces stopped updating
their input regions, all pointer input on the session was swallowed (the bar surfaces span the
entire monitor at the "top" layer; a frozen client cannot shrink its input region). Later the
same day the whole system OOMed and required a reboot.

Measured state at the freeze (cgroup `quantum.service`):

| Metric | Value |
|---|---|
| Total cgroup memory | 2.2 GB (over `MemoryHigh=2G`, under `MemoryMax=3G`) |
| `shmem` | 1.66 GB — **all `unevictable`** |
| `anon` | 526 MB (WebKitWebProcess heap 430 MB of it) |
| memfds held by quantumd | 6 × 1 GB sparse `memfd:state table` + `memfd:lp_dma_buf` |

Fresh-start baseline for comparison: ~1.0 GB total, ~840 MB shmem, ~135 MB anon. The same six
1 GB "state table" memfds exist from startup.

## Root cause

Two independent causes, one fix each.

### Cause 1: Vulkan renderer state pools (the memory)

GTK 4.18+ defaults GSK to the **Vulkan renderer**; the host runs GTK 4.22.4 and quantum sets no
`GSK_RENDERER`. On Intel graphics, Mesa's ANV Vulkan driver backs its state pools with 1 GB
sparse memfds named `state table` (string confirmed present in `libvulkan_intel.so`, absent from
`libgallium`). These pools grow monotonically with per-frame allocation churn and never shrink
until the VkDevice is destroyed. Six pools existed; resident pages grew from ~840 MB at startup
to 1.66 GB over 13 hours. The pages are GPU-pinned shmem — `unevictable`, unreclaimable.

### Cause 2: MemoryHigh with nothing to reclaim (the freeze)

`MemoryHigh=2G` tells the kernel to throttle-and-reclaim past 2 GB. The host has **no swap**, and
the dominant memory was unevictable shmem, so reclaim could make no progress — the kernel held
every thread in `mem_cgroup_handle_over_high` forever. `MemoryMax=3G` (which would OOM-kill and
trigger `Restart=on-failure`, self-healing) was never reached. `MemoryHigh` below the unevictable
working set converts a leak into a permanent freeze instead of a 2-second restart.

## Design

Edit `systemd.user.services.quantum` in `modules/features/home/quantum.nix`:

1. Add `"GSK_RENDERER=ngl"` to `Environment`. Forces GTK's OpenGL renderer; the iris GL driver
   does not allocate ANV state-table pools. Expected to remove most of the ~840 MB shmem baseline
   and its growth vector.
2. Remove `MemoryHigh = "2G"`. Keep `MemoryMax = "3G"` + `Restart = "on-failure"` as the guard:
   crossing the hard cap OOM-kills the cgroup and systemd respawns a clean daemon in ~2 s.
   Update the surrounding comment block to explain why MemoryHigh is absent (this file's comment
   currently argues FOR MemoryHigh; the incident disproved it for an unevictable working set).

## Verification (A/B) — required, the ngl claim is not yet proven

A first A/B attempt on 2026-09-02 was cut short by a system-wide OOM and reboot. Redo it:

1. Record baseline on the running Vulkan config:
   `grep -E '^(anon|shmem|unevictable) ' /sys/fs/cgroup/user.slice/user-1000.slice/user@1000.service/app.slice/quantum.service/memory.stat`
2. Apply the config change (`just build`, hand off to Ross for `just switch`), restart quantum.
3. Confirm the renderer took: `tr '\0' '\n' < /proc/$(systemctl --user show quantum.service -p MainPID --value)/environ | grep GSK_RENDERER`
   and confirm the six 1 GB `state table` memfds are gone from `/proc/<pid>/fd`.
4. Compare fresh-start shmem, then re-measure after 24 h uptime.
5. Visual check: bars, launcher, timers render correctly on both monitors; animations smooth.

## Acceptance criteria

- [ ] Fresh-start cgroup `shmem` drops by ≥ 500 MB versus the Vulkan baseline (~840 MB).
- [ ] No `memfd:state table` entries in quantumd's fd table.
- [ ] `MemoryHigh` absent from `systemctl --user show quantum.service` (shows `infinity`).
- [ ] Forced-leak drill: with a low temporary `MemoryMax` (runtime override), the service is
      OOM-killed and auto-restarts within 10 s — no D-state hang.
- [ ] No visual regressions across all quantum views.

## Risks

- ngl renderer differences (rare rendering glitches on some GTK versions). Fallback: `gl`
  (legacy GL renderer) as second choice; revert env var to return to Vulkan.
- If shmem savings are small, the leak is elsewhere (re-measure before proceeding to B/D).
- Losing MemoryHigh removes early throttling entirely; a fast leak now runs to 3 GB before dying.
  That is the intended trade: a clean kill beats a frozen session.

## Related

- Option B (WebKit memory pressure settings): `2026-09-02-memory-b-webkit-memory-pressure.md`
- Option C (scheduled recycle): `2026-09-02-memory-c-scheduled-recycle.md`
- Option D (web process recycling): `2026-09-02-memory-d-web-process-recycling.md`
