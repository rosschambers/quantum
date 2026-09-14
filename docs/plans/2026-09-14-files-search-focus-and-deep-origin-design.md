# Files View: Search Focus, Deep-Search Shortcut, and Result Origin — Design

Decisions settled in the playground (`docs/playgrounds/files-search-focus-origin-playground.html`)
on 2026-09-14. Frontend-only change set in `src/ui/plugins/files/views/files`.

## Decisions (settled)

1. **Filter-box focus state.** Focused OR non-empty filter → accent border, soft glow, and the
   box grows 200px → 320px (smooth transition). Today it has no focus state at all
   (`outline: none`, no `:focus-within` styling).
2. **Keyboard bindings.** `Ctrl+F` (or `Cmd+F`) focuses the filter input anywhere in the window,
   including while typing in it. `Ctrl+Shift+F` toggles deep search and focuses the filter. Both
   are added to `SHORTCUT_KEYS`/`SHORTCUT_GROUPS` in `shortcuts.ts` (the single source of truth —
   menus and the `?` cheat sheet read from it) and to `keymap.ts`.
3. **Deep-search result origin: grouped section headers.** In deep mode, results are grouped by
   containing folder: one header line per group showing the path **relative to the search root**,
   its files indented 12px under it. Headers stay at the standard **30px row height** so
   virtualization stays pure arithmetic; headers are non-selectable to the cursor but
   **clickable — clicking navigates the pane to that folder and clears the filter/deep state**.
   The single-parent case still shows its header (no collapse). Hover title on every result row
   shows the absolute path; Properties already shows it.
4. **Bug fix (independent, landed with this design):** "Open terminal here" on a file opens the
   entry's own parent directory (`parentOf(entry.path)`), not `ctx.path`. During deep search
   `ctx.path` is the search root, which is why the terminal opened the wrong folder. For
   directories "here" remains the directory itself.

## Design

### Keyboard plumbing

- `keymap.ts` gains two actions: `{ kind: 'focus-search' }` (`Ctrl/Cmd+F`, no shift) and
  `{ kind: 'toggle-deep-search' }` (`Ctrl/Cmd+Shift+F`).
- These two must fire **even when focus is in an input** — `App.svelte`'s `onKeyDown` currently
  returns early on any input target (`inInput` guard). Resolve these two combos before that
  guard; every other shortcut keeps the existing guard (Ctrl+A etc. must stay browser-native
  inside inputs).
- `Toolbar.svelte` exposes focus via a bindable counter or exposed action; App increments it,
  Toolbar focuses the input and selects any existing text. `Ctrl+Shift+F` toggles the pane's
  `deepSearch` exactly like the toolbar button (same code path) and then focuses.
- Cheat sheet: two new rows in the "View" group.

### Filter-box focus style

CSS-only on `.filter-wrap`: `:focus-within` and an `.active` class (non-empty filter) both get
accent border (`--color-accent`), a soft glow (`box-shadow: 0 0 0 1px accent, 0 0 10px
accent/25%`), and width 320px with a short transition. The toolbar is flex and the breadcrumb
truncates, so the growth has room.

### Grouped deep results

- Pure module (new `grouping.ts` next to `path.ts`, unit-testable): takes the sorted visible
  entries of a deep search plus the search root and returns a flat render list of
  `{ kind: 'header', path } | { kind: 'entry', entry }` items — groups ordered alphabetically by
  relative path from the search root, entries within a group keeping the pane's current sort.
- `FileList.svelte` renders headers as fixed-30px presentation rows (folder glyph + muted mono
  relative path) inside the same virtualization list — every render item keeps the same height so
  the scroll arithmetic is unchanged. Selection, cursor movement, Ctrl+A, and drag all operate on
  entries only; the cursor skips header items. Header click → navigate + clear filter/deep.
- Row hover `title` = absolute path (set unconditionally — free).

## Acceptance criteria

- [ ] `Ctrl+F` anywhere focuses the filter input (including from inside it); `Ctrl+Shift+F`
      toggles deep on AND off, focus lands/stays in the filter. Cheat sheet shows both.
- [ ] Focused or non-empty filter box shows accent border + glow + 320px width; state clears when
      empty and unfocused.
- [ ] Deep results render grouped: header per containing folder (relative path), rows indented
      12px, headers at 30px height, alphabetical group order, current sort within groups.
- [ ] Cursor skips headers; Ctrl+A selects entries only; clicking a header navigates to that
      folder and clears filter/deep state; single-parent results still show the header.
- [ ] Hover title on a result row shows the absolute path; Properties shows it too.
- [ ] Open-terminal-here from a deep-search file opens the file's parent directory (pinned by
      tests in `menus.test.ts`, landed).

## Tests

- `keymap.test.ts`: the two new bindings (ctrl and meta), and shift variants not stealing them.
- `App.test.ts`: Ctrl+F focuses the input; Ctrl+Shift+F toggles deep both ways and focuses;
  shortcuts fire while focus is in the filter input.
- `grouping.test.ts`: pure grouping — group order, relative paths, single group, sort-within-group.
- `FileList.test.ts`: headers render at fixed height, are non-selectable, cursor skip works, click navigates.
- `menus.test.ts`: landed with the fix (`parentOf` for files; directory behavior unchanged).
