# Clipboard history in the launcher — design

Status: approved by Ross, 2026-10-15; preview layout settled in the playground.

## Problem

Quantum already records clipboard history and serves it in the launcher: `;` searches the
last 100 copies, newest first, and Enter copies the chosen entry again (the clipboard
subsystem in `AGENTS.md`). Ross forgot it existed. There is no direct way to reach it, a
long copy shows as one truncated line, and two other recorders run beside it:

| Recorder | Started by | Holds | Used by anything on screen |
|----------|-----------|-------|----------------------------|
| quantum `ClipboardWatcher` | `quantumd` | 100 text entries, Oct 11 → now | yes, launcher `;` |
| cliphist | `hyprland.nix:275-276` (`niri.nix:204-205` for niri) | 750 entries, January → now, `~/.cache/cliphist/db` | no |
| elephant (walker's backend) | `head.nix:41` user service, `walker --gapplication-service` at `hyprland.nix:277` | `~/.cache/elephant/clipboard.gob` | no keybind; only eww scripts, and eww is not running |

None of the three skips secrets. Today both quantum's and cliphist's history hold one
token-shaped string each, and agent instructions tell agents to put secrets on the clipboard
with plain `wl-copy`.

## Decisions (from Ross, 2026-10-15)

- Keep history in the launcher and improve it. No separate window.
- Retention stays at 100 entries.
- Remove cliphist and walker/elephant; do not import their history.
- x1 only; no sync.

## Design

### 1. Super+V opens the launcher in clipboard mode

- Hyprland bind (config repo `hyprland.nix`): `$mod, V, exec, quantumctl show launcher --args '{"query":";"}'`.
  Super+V is currently unbound.
- Use the existing view-argument path. The registry injects `window.__quantum_args` on
  **every** open (`registry.rs:828`) and injects `null` when an open has no arguments, so
  Super+L still opens an empty launcher.
- Launcher `App.svelte`: the mount search and `requeryOnShow` start from
  `initialQuery()` instead of `''`. `initialQuery()` returns `__quantum_args.query` when it is a
  string, otherwise `''`. Reading the global goes through one small helper in `lib/`, with tests.
- The input shows `;` with the cursor after it, so typing filters straight away.

Acceptance:
- Super+V opens the launcher showing `;` and the clipboard list. Super+L opens it empty
  with apps, including right after a Super+V.
- With the launcher already open in app mode, Super+V switches it to clipboard mode.

Risk: injecting the arguments (`evaluate_javascript`) and the window's focus event could
arrive in either order. Verify on the live launcher first. If it races, read the arguments
on the next animation frame. Choose the fix from what the live test shows, not from a guess.

### 2. A preview of the highlighted entry

- In clipboard mode (`;` prefix, provider `clipboard`), the launcher shows a preview pane
  for the highlighted entry:
  - **Text:** the full text, whitespace and line breaks preserved, monospace, scrolling inside
    its own pane. Above it, a metadata line: age, size, and line count.
  - **Image:** the stored thumbnail scaled to fit, plus its dimensions. No full-resolution
    fetch: there were zero image copies in the last four days (quantum) and none in cliphist
    since January.
  - **File or binary:** title and MIME type only.
- **Data source:** a text result's primary action is already `{kind: "copy", data: {text}}`
  carrying the full text (`clipboard.rs:141`). The preview renders exactly that, so it shows
  precisely what Enter will copy. No new IPC and no duplicated field.
- **Layout (settled in `docs/playgrounds/2026-10-15-clipboard-preview-layout.html`):**
  - **Card size:** in `;` clipboard mode only, the card widens from 600px to 900px and is held
    at a fixed 720px height (today's 60% maximum on the 1920×1200 screen), so it does not
    resize as the highlight moves. Every other mode keeps the 600px card.
  - **Split:** the result list takes 50% of the width on the left. The preview fills the right
    half, separated by a 1px `--color-border` line.
  - **Preview text:** monospace (`--font-mono`), 12px, long lines wrapped (`pre-wrap`,
    `break-word`). The preview scrolls on its own and resets to the top when the highlight
    changes.
  - **Details line:** above the preview text, 12px `--color-muted`. It shows age · size · line
    count for text, age · size · dimensions for images, and age · MIME type for files.
  - **Images and files:** an image shows its stored thumbnail scaled to the pane width; a file
    shows its full path.
- Other prefix modes are unchanged.

Acceptance:
- Moving the highlight with the arrow keys updates the preview immediately.
- A 200-line copy is fully readable by scrolling the preview. The list row is unchanged.
- The card has a fixed size in clipboard mode: it does not resize as the highlight moves.

### 3. Do not record secrets

- `ClipboardWatcher::handle_change` already lists the offered MIME types per change. If they
  include `x-kde-passwordManagerHint`, drop the change and do not record it. That hint is the
  same signal `wl-paste` turns into `CLIPBOARD_STATE=sensitive`. `wl-copy --sensitive` sets it,
  and so do password managers that follow the KDE convention.
- Log at debug level only ("skipped sensitive clipboard change"), never the content.
- Agent rule: everywhere agents are told to "put secrets on the clipboard with `wl-copy`",
  change it to `wl-copy --sensitive`. That covers the global `AGENTS.md` in the opencode
  profile and Ross's brain preference about showing setup steps in qv.
- Ross removes the existing token-shaped entry with the `;` delete action (or the agent does,
  after showing which one).

Acceptance:
- `printf 'not-a-secret' | wl-copy` appears in `;`. `printf 'hunter2' | wl-copy --sensitive`
  does not, and `clipboard.json` does not contain it.

### 4. Remove cliphist and walker/elephant (config repo)

- `hyprland.nix`: delete the three `exec-once` lines 275-277.
- `niri.nix`: delete the two cliphist `spawn-at-startup` lines.
- `head.nix`: delete `elephantProviders`, the `xdg.configFile` elephant providers, the
  `systemd.user.services.elephant` unit, the `walker.nix` import, and the `elephant` and
  `cliphist` packages. Keep `wl-clipboard`: quantum and agents use it.
- Delete `modules/features/home/walker.nix`.
- Leave eww's `reboot-menu.sh` / `launcher.sh` alone. They still call walker, but eww is not
  running. Capture "eww config is dead" as a separate inbox item.
- After Ross runs `just switch`: kill the stale `wl-paste … cliphist store` processes (started
  by Hyprland at login, so a switch does not stop them). Then delete their stores:
  - `~/.cache/cliphist/db`: 16 MB, 750 entries going back to January. This is the history
    being dropped, and it holds at least one secret.
  - `~/.cache/elephant/`: clipboard, file index and run history for walker, which is gone.
  - `~/.config/walker/` and `~/.config/elephant/`: generated configuration for removed
    programs.

Acceptance:
- `just build` and `just check` pass.
- After the switch and cleanup, `pgrep -af 'cliphist|elephant|walker'` is empty and quantum's
  watcher is the only `wl-paste --watch`.

## Out of scope

Retention changes, a dedicated history window, pinning, sync, full-resolution image preview.

Known side issue, captured separately: `inject_view_args` connects a new `load-changed`
handler on every open of a warm view, so these closures pile up on the launcher over time.

## Testing

- Rust: unit test that a type list containing `x-kde-passwordManagerHint` is not recorded and
  a normal text list is, in `clipboard_watcher.rs` beside `parse_list_types`.
- Launcher vitest:
  - `initialQuery` for a string, a missing value and a non-string.
  - The reopen path uses the arguments.
  - The preview renders the copy text for a clipboard match and is absent for other providers.
  - The metadata line.
- Headless render (the `AGENTS.md` recipe) of the built launcher in clipboard mode, to check
  layout and truncation.
- Live on x1 after the switch: the acceptance lines above, one at a time.

## Deployment

1. Quantum: commit per task, push `main`.
2. Config repo: bump the `quantum` input, apply the Hyprland/niri/head/walker changes, then
   run `just build` and `just check`. Prove the new view is in the build with
   `grep -a` on `.quantumd-wrapped`.
3. Ross runs `just switch`. The agent then does the process kill, the cache deletion and the
   live checks.
