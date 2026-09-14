# Files View: Search Focus, Deep-Search Shortcut, Grouped Result Origin — Implementation Plan

> **For OpenCode:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

Design: `docs/plans/2026-09-14-files-search-focus-and-deep-origin-design.md` (settled in
`docs/playgrounds/files-search-focus-origin-playground.html`). The open-terminal-here fix already
landed (`parentOf` for file entries, tests pinned in `menus.test.ts`) — it is not re-planned here.

**Goal:** Ctrl+F focuses the filter box with a visible accent state; Ctrl+Shift+F toggles deep search; deep-search results group under folder headers so each result shows where it came from.

**Architecture:** All changes are frontend-only in the `files` view package
(`src/ui/plugins/files/views/files`). Shortcuts extend the pure resolver (`keymap.ts`) + the
cheat-sheet source (`shortcuts.ts`) + App dispatch (the two search actions must fire even while
an input has focus). Grouping is pure ordering in `PaneState`: `visibleEntries()` returns entries
already in grouped order when deep search is active, and a new derived `listItems()` interleaves
fixed-30px header items so `FileList`'s index-based virtualization arithmetic is untouched. The
keyboard cursor keeps indexing entries only (headers are skipped); selection stays path-keyed.

**Tech Stack:** Svelte 5 (runes), vitest + @testing-library/svelte. Tests run from the view package:
`cd src/ui/plugins/files/views/files && pnpm exec vitest run`. No Rust/IPC changes.

**Assumptions:**
- `keymap.ts` `resolveShortcut` currently returns null for Ctrl+F / Ctrl+Shift+F (verified —
  only a/c/x/v/d/h and ctrl+shift+n exist); the two new kinds are added to its union type.
- App's `onKeyDown` early-returns on any input target (`inInput` guard, ~line 442). The two new
  search actions must resolve BEFORE that guard; every other shortcut keeps the guard so Ctrl+A
  etc. stay browser-native inside inputs.
- The search root for relative headers is `pane.path` (the directory deep search was run on).
- Group order: alphabetical by path relative to the search root; entries directly in the root
  group under header label `.` (meaning "here"). Within a group, the pane's current sort order
  is preserved. A single group still renders its header (no collapse — settled decision).
- Header rows are presentation-only: never selectable, the keyboard cursor skips them, click
  navigates the pane to that folder and clears filter + deep-search state.
- The accent focus state uses theme tokens only (`--color-accent`, `--color-border`); the width
  grows 200px → 320px on focus-within OR non-empty filter (settled in playground).

---

### Task 1: Keymap — Ctrl+F and Ctrl+Shift+F actions

**Files:**
- Modify: `src/ui/plugins/files/views/files/src/lib/keymap.ts`
- Test: `src/ui/plugins/files/views/files/src/lib/keymap.test.ts`

**Acceptance Criteria:**
- [ ] `ShortcutAction` union gains `{ kind: 'focus-search' }` and `{ kind: 'deep-search' }`.
- [ ] `resolveShortcut` maps Ctrl/Cmd+F (no shift) → `focus-search`, Ctrl/Cmd+Shift+F → `deep-search`.
- [ ] Existing mappings unchanged; existing tests pass.

**Step 1: Write the failing tests** — add to the ctrl section of `keymap.test.ts`:

```ts
test('search shortcuts', () => {
    expect(resolveShortcut(key({ key: 'f', ctrlKey: true }))).toEqual({ kind: 'focus-search' });
    expect(resolveShortcut(key({ key: 'F', metaKey: true, shiftKey: true })))
        .toEqual({ kind: 'deep-search' });
});
```

**Step 2:** Run `pnpm exec vitest run src/lib/keymap.test.ts` — expect FAIL (both return null).

**Step 3: Implement.** In `keymap.ts`: add the two union members, then inside
`resolveShortcut` extend the `if (control && !event.shiftKey)` switch with
`case 'f': return { kind: 'focus-search' };` and extend the ctrl+shift block to
`if (control && event.shiftKey && (lower === 'n' || lower === 'f')) { return lower === 'n' ? { kind: 'new-folder' } : { kind: 'deep-search' }; }`.

**Step 4:** Re-run — expect PASS. **Step 5: Commit** `feat(files): keymap bindings for focus-search and deep-search`.

---

### Task 2: Cheat sheet rows (single source of truth)

**Files:**
- Modify: `src/ui/plugins/files/views/files/src/lib/shortcuts.ts`
- Test: `src/ui/plugins/files/views/files/src/lib/shortcuts.test.ts`

**Acceptance Criteria:**
- [ ] `SHORTCUT_KEYS` gains `focusSearch: 'Ctrl+F'` and `deepSearch: 'Ctrl+Shift+F'`.
- [ ] The `View` group in `SHORTCUT_GROUPS` gains rows "Focus filter" and "Toggle deep search", referencing the constants (no literals).
- [ ] Existing tests pass; new assertions added to `shortcuts.test.ts`.

**Step 1:** failing test asserting the two hints exist in the View group via `SHORTCUT_KEYS`.
**Step 2:** verify red. **Step 3:** implement. **Step 4:** green. **Step 5: Commit** `feat(files): cheat-sheet rows for the search shortcuts`.

---

### Task 3: Wire the actions — focus plumbing + accent state in Toolbar, dispatch in App

**Files:**
- Modify: `src/ui/plugins/files/views/files/src/lib/Toolbar.svelte`
- Modify: `src/ui/plugins/files/views/files/src/App.svelte` (`onKeyDown`, `dispatchShortcut`, Toolbar call site)
- Test: `src/ui/plugins/files/views/files/src/App.test.ts`, `src/lib/Toolbar.test.ts`

**Acceptance Criteria:**
- [ ] Ctrl/Cmd+F anywhere (INCLUDING while focus is in the filter or location input) focuses the filter input and selects its text; existing `inInput` guard still swallows every other shortcut inside inputs.
- [ ] Ctrl/Cmd+Shift+F calls the existing `toggleDeep()` (same code path as the toolbar button — toggles ON *and* OFF) and then focuses the filter.
- [ ] Toolbar exposes focus via a numeric `focusSignal` prop; an `$effect` focuses + selects the input when it changes (Svelte 5 runes — no `onMount`, per repo rule).
- [ ] `.filter-wrap:focus-within` and a new `.filter-wrap.active` (non-empty filter) both get: accent border, `box-shadow: 0 0 0 1px var(--color-accent), 0 0 10px rgb(166 227 161 / 25%)`, width 320px, transition 120ms. State clears when empty and unfocused. Theme tokens only (sycamore accent `#a6e3a1`).

**Tests:** in `App.test.ts` — render App, fire `keyDown(window, { key: 'f', ctrlKey: true })`, assert `document.activeElement` is the filter input; fire Ctrl+Shift+F and assert the deep toggle shows active + focus moved into the input; fire Ctrl+F from inside the input (focus the input first) and assert it stays. In `Toolbar.test.ts` — incrementing `focusSignal` focuses the rendered input.

**Implement, verify green, commit** `feat(files): Ctrl+F focus state and Ctrl+Shift+F deep-search toggle`.

---

### Task 4: Grouped ordering in PaneState (pure logic)

**Files:**
- Modify: `src/ui/plugins/files/views/files/src/lib/paneState.svelte.ts`
- Test: `src/ui/plugins/files/views/files/src/lib/paneState.test.ts`

**Acceptance Criteria:**
- [ ] New exported type `ListHeader { kind: 'header'; path: string }` (absolute path of the group directory) and `ListItem = ListHeader | { kind: 'entry'; entry: FileEntry }`.
- [ ] `visibleEntries()` behavior UNCHANGED for normal browsing; when `deepSearch && filter.trim() !== ''`, it returns entries regrouped: groups sorted alphabetically by path relative to `this.path` (root-level group label `.`), current sort preserved within each group. Cursor/selectAll keep indexing this list, so grouping must happen here — not in the view.
- [ ] New derived method `listItems(): ListItem[]`: walks `visibleEntries()` and inserts a header item before the first entry of each group (only while grouped). Non-grouped → entries only, no headers.
- [ ] Tests: group order, within-group sort preserved (name AND size), single-group still emits one header, non-deep mode returns entries unchanged, root-level group label is `.`.

**Steps:** failing tests → implement (`parentOf` from `path.ts` computes each entry's group) → green → commit `feat(files): deep-search results ordered by containing folder in pane state`.

---

### Task 5: FileList headers + row hover path

**Files:**
- Modify: `src/ui/plugins/files/views/files/src/lib/FileList.svelte`
- Modify: `src/ui/plugins/files/views/files/src/lib/Row.svelte` (hover title only)
- Test: `src/ui/plugins/files/views/files/src/lib/FileList.test.ts`, `src/lib/Row.test.ts`

**Acceptance Criteria:**
- [ ] `FileList` renders a flat list of `ListItem`s (`listItems()`); header items render a fixed **30px** row (same `ROW_HEIGHT` — virtualization arithmetic untouched): folder glyph + muted monospace relative path, using theme tokens only.
- [ ] Rows under their header are indented 12px; headers sit at the left edge.
- [ ] Headers are presentation-only: no selection on click, no drag source; the keyboard cursor (App-side) skips non-entry items — verify `selectRange`/cursor math still operate on entry-index space by keeping `visibleEntries()` entry-only OR mapping index→item in FileList (whichever Task 4 chose; tests pin the behavior).
- [ ] Clicking a header calls new callback `onGroupNavigate?(absolutePath)`; App wires it to navigate + clear filter + clear `deepSearch`.
- [ ] Every row's name span gets `title={entry.path}` (absolute path, unconditional — free).

**Tests:** FileList renders one header per group with the relative label; header click fires the callback and never touches selection; row hover title carries the absolute path; existing virtualization/selection tests pass unchanged.

**Steps:** failing tests → implement → green → commit `feat(files): deep-search group headers with origin paths`.

---

### Task 6: Full suite + live check

- [ ] `cd src/ui/plugins/files/views/files && pnpm exec vitest run` — all green (was 254 before this plan).
- [ ] `pnpm --filter default-panel-files build` clean.
- [ ] Live check via dev daemon (`QUANTUM_PLUGIN_DIR=src/ui/plugins`): Ctrl+F glows + focus; Ctrl+Shift+F toggles deep both ways; grouped headers render with relative paths; header click navigates and clears search; open-terminal-here on a deep result opens the file's parent.
