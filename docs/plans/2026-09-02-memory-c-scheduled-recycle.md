# Memory Option C: Scheduled daily service recycle

Status: proposed. Implementation lands in the config repo
(`code/infra/config/modules/features/home/quantum.nix`).

## Background

See `2026-09-02-memory-a-gsk-renderer-and-memoryhigh.md` for the incident. Both measured growth
vectors (Vulkan state pools: +800 MB over 13 h; WebKit anon heap: +390 MB over 13 h) reset fully
on service restart, and a restart takes ~2 s, respawning fresh bars without touching Hyprland or
application windows. A scheduled recycle is therefore a cheap, guaranteed backstop for whatever
growth options A and B do not eliminate — including leaks that do not exist yet.

## Design

Add to `systemd.user.services.quantum` Service section:

```nix
RuntimeMaxSec = "86400";
Restart = "always";
```

- `RuntimeMaxSec=86400` — systemd stops the service after 24 h of runtime.
- `Restart = "always"` replaces the current `Restart = "on-failure"`: a `RuntimeMaxSec`
  expiry counts as a *clean* stop-by-timeout (`SIGTERM`), which `on-failure` would NOT restart
  after. `always` covers both the daily expiry and crash cases. Keep `RestartSec = 2`.

### Restart-timing refinement (optional, recommended)

A fixed 24 h period drifts through the day and will eventually land mid-use. Two mitigations,
pick one:

1. **Accept the flicker.** Widgets disappear for ~2 s once a day. Simplest; probably fine.
2. **Timer-driven restart at a quiet hour instead of RuntimeMaxSec.** A
   `systemd.user.timers` unit firing `systemctl --user try-restart quantum.service` at 04:30
   local daily. Deterministic timing, never interrupts active use; slightly more config. If
   chosen, skip `RuntimeMaxSec` entirely and keep `Restart = "on-failure"`.

Recommendation: option 2 (quiet-hour timer). The 24 h drift of RuntimeMaxSec means the restart
eventually happens exactly while the launcher is open, and a dismissed launcher mid-keystroke is
an avoidable annoyance for one extra unit of config.

## Interaction with other options

- Independent of A and B; safe to land in the same change as A (same file).
- If D (in-process web process recycling) ever lands, this remains useful as the outer guard —
  D cannot reset the Vulkan/GTK side of the daemon, only WebKit.

## Acceptance criteria

- [ ] Service restarts daily at the configured time (verify over 3 days:
      `journalctl --user -u quantum.service --since '3 days ago' | grep -E 'Started|Stopping'`).
- [ ] Cgroup memory returns to fresh baseline (~1 GB on Vulkan, less after option A) after each
      recycle.
- [ ] Bars, timers, clock respawn automatically on both monitors after recycle; launcher opens
      normally afterward.
- [ ] A crash mid-day still auto-restarts (unchanged behaviour).
- [ ] Restart does not interrupt active use (if timer variant: fires at 04:30 local).

## Risks

- Anything with in-memory-only state in a warm view (running timers in the timers plugin?) loses
  it on recycle. Check whether the timers plugin persists state; if not, that is an argument for
  the quiet-hour timer variant and/or persisting timer state — verify before enabling.
- `Restart=always` + a crash-loop bug means infinite restart churn; `StartLimitIntervalSec`
  defaults provide some protection, and `RestartSec=2` keeps it visible rather than hot.

## Related

- `2026-09-02-memory-a-gsk-renderer-and-memoryhigh.md` (same file, land together)
- `2026-09-02-memory-b-webkit-memory-pressure.md`
- `2026-09-02-memory-d-web-process-recycling.md`
