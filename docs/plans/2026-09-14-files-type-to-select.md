# Files View: Type-to-Select (Highlight + Enter) Implementation Plan

> **For OpenCode:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make typing in a file-explorer pane highlight the first filter match and make Enter open the highlighted entry, so "type a few letters, press Enter" reliably enters the folder.

**Architecture:** The type-to-filter feature (commit `451bb286`) replaced the original type-ahead (`typeAhead()`, commit `7bf5dbcf`) but only kept the *narrowing* half — it dropped the *select-first-match* half, and the dead function was left behind in `App.svelte`. This plan restores the selection sync inside `handleFilterInput` (so the toolbar filter box benefits identically), makes Enter clamp its cursor lookup against the filtered list, and gives the toolbar filter input an Enter handler. All changes are frontend-only, in the `files` view package; no Rust/IPC changes.

**Tech Stack:** Svelte 5 (runes), vitest + @testing-library/svelte. Tests run from the view package directory: `cd src/ui/plugins/files/views/files && pnpm exec vitest run`.

**Assumptions:**
- The intended UX is: typing narrows the list AND selects the first match (so Enter opens it); clearing the filter clears the selection; a click/arrow selection still works exactly as before.
- Only the toolbar filter input needs an Enter handler; the breadcrumb already navigates on Enter, and window-level keydown ignores events whose target is an input (`inInput` guard in `App.svelte`) — that guard stays.
- `typeAhead()` has no remaining callers (verified by grep across `src/`); deleting it is safe.

## Root Cause (for the record)

1. **`typeAhead()` is dead code.** `src/ui/plugins/files/views/files/src/App.svelte:514` defines it (buffer + select first prefix match + cursor sync), but commit `451bb286` replaced its call site with `handleFilterInput(pane.filter + event.key)`. Nothing ever calls it. So typing filters the list but never selects anything — nothing highlights.
2. **Enter acts on a stale cursor.** The Enter branch (`App.svelte:471`) opens `pane.visibleEntries()[cursors[index]]`. Filtering never moves or clamps `cursors` (only `toggleHidden` calls `clampCursor`). Enter therefore "works" only when the match coincidentally sits at the old cursor index (usually 0 after navigating fresh — hence "sometimes"), and silently does nothing when the filtered list is shorter than the stale index.
3. **The toolbar filter input has no Enter handler** (`Toolbar.svelte` forwards `oninput` only). When focus is in the filter box, typing filters fine but Enter is swallowed by the `inInput` guard — so a narrowed-to-one list still cannot be entered with Enter.

---

### Task 1: Sync selection to the first visible match when the filter changes

**Files:**
- Modify: `src/ui/plugins/files/views/files/src/App.svelte` (`handleFilterInput`, ~line 717)
- Test: `src/ui/plugins/files/views/files/src/App.test.ts` (existing `describe('App type-to-filter')` suite, ~line 94)

**Acceptance Criteria:**
- [ ] After typing characters that match at least one entry, the FIRST visible (filtered + sorted) row in the active pane carries the `.sel` class.
- [ ] Typing Backspace re-syncs: after `d`, `o`, Backspace the first visible row of the `d`-filtered list is selected.
- [ ] Clearing the filter to empty clears the selection (no `.sel` rows) — preserves the existing Escape-clears-selection behavior in the test at `App.test.ts:159`.
- [ ] Cursor and anchor for the active pane are synced to index 0 on a match (so arrow keys continue from the highlight).
- [ ] Existing tests in `App.test.ts` still pass unchanged.
- [ ] No changes to files outside the list above.

**Step 1: Write the failing test**

Add to the `describe('App type-to-filter')` suite in `App.test.ts` (reuse its `filterEntries()` and `activeRow()` helpers; selection renders as `.sel` per `Row.svelte`):

```ts
it('typing selects the first match so Enter can open it', async () => {
    const ipc = createFakeIpc(filterEntries());
    const { container } = render(App, { props: { ipc } });

    await vi.waitFor(() => {
        expect(activeRow(container, `${HOME}/music`)).not.toBeNull();
    });

    await fireEvent.keyDown(window, { key: 'd' });
    await fireEvent.keyDown(window, { key: 'o' });

    // The narrowed list is [docs, downloads] (folders first, name order);
    // the first match must carry the selection class.
    await vi.waitFor(() => {
        expect(activeRow(container, `${HOME}/docs`)?.classList.contains('sel')).toBe(true);
    });
});

it('Backspace re-syncs the selection to the first match of the shortened filter', async () => {
    const ipc = createFakeIpc(filterEntries());
    const { container } = render(App, { props: { ipc } });

    await vi.waitFor(() => {
        expect(activeRow(container, `${HOME}/music`)).not.toBeNull();
    });

    await fireEvent.keyDown(window, { key: 'd' });   // matches docs + downloads
    await fireEvent.keyDown(window, { key: 'o' });   // matches downloads only
    await vi.waitFor(() => {
        expect(activeRow(container, `${HOME}/downloads`)?.classList.contains('sel')).toBe(true);
    });

    await fireEvent.keyDown(window, { key: 'Backspace' }); // back to 'd' — first match is docs
    await vi.waitFor(() => {
        expect(activeRow(container, `${HOME}/docs`)?.classList.contains('sel')).toBe(true);
    });
});

it('clearing the filter clears the selection', async () => {
    const ipc = createFakeIpc(filterEntries());
    const { container } = render(App, { props: { ipc } });

    await vi.waitFor(() => {
        expect(activeRow(container, `${HOME}/music`)).not.toBeNull();
    });

    await fireEvent.keyDown(window, { key: 'd' });
    await vi.waitFor(() => {
        expect(activeRow(container, `${HOME}/docs`)?.classList.contains('sel')).toBe(true);
    });

    await fireEvent.keyDown(window, { key: 'Backspace' }); // filter empty -> selection cleared
    await vi.waitFor(() => {
        expect(container.querySelector('.pane:not(.inactive-pane) .frow.sel')).toBeNull();
    });
});
```

**Step 2: Run tests to verify they fail**

Run: `pnpm exec vitest run src/App.test.ts` (from `src/ui/plugins/files/views/files`).
Expected: the three new tests FAIL (no row carries `.sel`); pre-existing tests pass.

**Step 3: Implement selection sync in `App.svelte`**

Replace `handleFilterInput` (~line 717) with:

```ts
function handleFilterInput(value: string): void {
    const pane = active;
    const changed = value !== pane.filter;
    pane.filter = value;
    if (pane.deepSearch && changed) {
        void loadPane(pane).then(syncFilterSelection);
    } else {
        syncFilterSelection();
    }
}

/**
 * Point the selection and keyboard cursor at the first visible match after a
 * filter change, so typed text highlights a target Enter can open. An empty
 * filter clears the selection (matching Escape); no matches clamps without
 * selecting.
 */
function syncFilterSelection(): void {
    const pane = active;
    const index = activePaneIndex;
    if (pane.filter === '') {
        pane.clearSelection();
        clampCursor(index);
        return;
    }
    const visible = pane.visibleEntries();
    if (visible.length > 0) {
        pane.selectOnly(visible[0].path);
        anchors[index] = 0;
        cursors[index] = 0;
    } else {
        clampCursor(index);
    }
}
```

**Step 4: Run tests to verify they pass**

Run: `pnpm exec vitest run src/App.test.ts`. Expected: all pass, including the pre-existing type-to-filter tests.

**Step 5: Commit**

```bash
git add src/ui/plugins/files/views/files/src/App.svelte src/ui/plugins/files/views/files/src/App.test.ts
git commit -m "fix: select the first filter match so typed text highlights a target"
```

---

### Task 2: Clamp the Enter cursor against the filtered list

**Files:**
- Modify: `src/ui/plugins/files/views/files/src/App.svelte` (Enter branch in `onKeyDown`, ~line 471)
- Test: `src/ui/plugins/files/views/files/src/App.test.ts`

**Acceptance Criteria:**
- [ ] Enter opens the entry at the clamped cursor index — never `undefined`.
- [ ] Regression scenario: select the LAST row (click), then type a letter that narrows the list to one different entry; Enter opens that entry, not nothing.
- [ ] Existing keyboard-navigation tests still pass.

**Step 1: Write the failing test**

Add to the `describe('App type-to-filter')` suite (a stale cursor at the last row must not strand Enter):

```ts
it('Enter opens the first match after a stale cursor from a prior selection', async () => {
    const ipc = createFakeIpc(filterEntries());
    const { container } = render(App, { props: { ipc } });

    await vi.waitFor(() => {
        expect(activeRow(container, `${HOME}/music`)).not.toBeNull();
    });

    // Click the last row (notes.txt) so the cursor sits at index 3.
    await fireEvent.click(activeRow(container, `${HOME}/notes.txt`)!);

    // Type a prefix matching exactly one entry: the selection sync (Task 1)
    // moves the highlight to `docs`, so Enter must open `docs`.
    await fireEvent.keyDown(window, { key: 'd' });
    await fireEvent.keyDown(window, { key: 'o' });
    await vi.waitFor(() => {
        expect(activeRow(container, `${HOME}/docs`)?.classList.contains('sel')).toBe(true);
    });

    await fireEvent.keyDown(window, { key: 'Enter' });

    // Navigating into a directory lists it (see the double-click test pattern).
    await vi.waitFor(() => {
        expect(ipc.list).toHaveBeenCalledWith(`${HOME}/docs`);
    });
});
```

**Step 2: Run test to verify it fails**

Run: `pnpm exec vitest run src/App.test.ts`. Expected: the new test FAILS *only if Task 1 was skipped*; with Task 1 done it may already pass — in that case additionally assert the clamp directly by selecting the last row then typing a prefix matching only files whose index < cursor is impossible after Task 1; keep the test as the regression guard either way (it pins the combined behavior).

**Step 3: Implement the clamp**

In the Enter branch of `onKeyDown` in `App.svelte` (~line 471), replace:

```ts
} else if (event.key === 'Enter') {
    const entry = pane.visibleEntries()[cursors[index]];
    if (entry !== undefined) {
        openEntry(index, entry);
    }
}
```

with:

```ts
} else if (event.key === 'Enter') {
    const visible = pane.visibleEntries();
    if (visible.length > 0) {
        clampCursor(index);
        const entry = visible[cursors[index]];
        openEntry(index, entry);
    }
}
```

**Step 4: Run tests to verify they pass**

Run: `pnpm exec vitest run src/App.test.ts`. Expected: all pass.

**Step 5: Commit**

```bash
git add src/ui/plugins/files/views/files/src/App.svelte src/ui/plugins/files/views/files/src/App.test.ts
git commit -m "fix: clamp the keyboard cursor before Enter opens an entry"
```

---

### Task 3: Enter in the toolbar filter box opens the highlighted entry

**Files:**
- Modify: `src/ui/plugins/files/views/files/src/lib/Toolbar.svelte` (filter input, ~line 100; Props interface)
- Modify: `src/ui/plugins/files/views/files/src/App.svelte` (`<Toolbar ... />` wiring, ~line 886)
- Test: `src/ui/plugins/files/views/files/src/lib/Toolbar.test.ts`

**Acceptance Criteria:**
- [ ] `Toolbar` gains an optional `onEnter?: () => void` prop fired on `keydown` with `key === 'Enter'` while the filter input has focus.
- [ ] Other keys inside the input still do nothing at the App level (the `inInput` guard in `onKeyDown` is unchanged — typing must not double-filter).
- [ ] App wires `onEnter` to a shared `openCursorEntry()` helper used by both the window Enter branch and the toolbar.
- [ ] A Toolbar unit test fires `keyDown(Enter)` on `.filter-input` and asserts `onEnter` was called; other keys do not fire it.
- [ ] No behavior change for the breadcrumb's own Enter handling.

**Step 1: Write the failing test**

Add to `Toolbar.test.ts` (follow its existing render/props pattern):

```ts
it('fires onEnter when Enter is pressed in the filter input', async () => {
    const onEnter = vi.fn();
    const { container } = render(Toolbar, { props: toolbarProps({ onEnter }) });
    const input = container.querySelector('.filter-input') as HTMLInputElement;

    await fireEvent.keyDown(input, { key: 'Enter' });
    expect(onEnter).toHaveBeenCalledTimes(1);

    await fireEvent.keyDown(input, { key: 'a' });
    expect(onEnter).toHaveBeenCalledTimes(1);
});
```

**Step 2: Run test to verify it fails**

Run: `pnpm exec vitest run src/lib/Toolbar.test.ts` (from the view package). Expected: FAIL — prop not supported yet.

**Step 3: Implement**

`Toolbar.svelte`: add `onEnter?: () => void;` to Props, destructure it, and on the filter `<input>` add:

```svelte
onkeydown={(event) => {
    if (event.key === 'Enter') {
        event.preventDefault();
        onEnter?.();
    }
}}
```

`App.svelte`: extract the Enter branch body (from Task 2) into `function openCursorEntry(): void`, call it from both the window keydown Enter branch and `<Toolbar ... onEnter={openCursorEntry} />`.

**Step 4: Run tests to verify they pass**

Run: `pnpm exec vitest run src/lib/Toolbar.test.ts src/App.test.ts`. Expected: all pass.

**Step 5: Commit**

```bash
git add src/ui/plugins/files/views/files/src/lib/Toolbar.svelte src/lib/Toolbar.test.ts src/App.svelte
git commit -m "feat: Enter in the files filter box opens the highlighted entry"
```

---

### Task 4: Delete the dead `typeAhead` code and stale comment

**Files:**
- Modify: `src/ui/plugins/files/views/files/src/App.svelte` (delete `typeAhead`, ~lines 513-531, and its `typeaheadBuffer` / `typeaheadTimer` declarations)
- Modify: `src/ui/plugins/files/views/files/src/lib/keymap.ts` (header comment line 5: "type-ahead" → "type-to-filter")

**Acceptance Criteria:**
- [ ] `grep -rn typeAhead src/` returns nothing.
- [ ] `keymap.ts` header comment no longer claims type-ahead lives in `onKeyDown`.
- [ ] The keymap comment reflects reality: management shortcuts live in `resolveShortcut`; arrows/Enter/Tab/Ctrl+L/type-to-filter stay in `onKeyDown`.

**Step 1: Delete dead code and fix the comment** (no behavior change; the selection-sync test from Task 1 covers the resurrected behavior).

**Step 2: Run the full view suite**

Run: `pnpm exec vitest run` (from the view package). Expected: all green.

**Step 3: Commit**

```bash
git add src/ui/plugins/files/views/files/src/App.svelte src/ui/plugins/files/views/files/src/lib/keymap.ts
git commit -m "refactor: remove dead typeAhead superseded by filter selection sync"
```

---

## Manual Verification (after all tasks)

1. `pnpm --filter @quantum/client build`, then `pnpm --filter default-panel-files build` from `src/ui`; run the dev daemon (`QUANTUM_PLUGIN_DIR=src/ui/plugins ./scripts/devsh.sh ./target/debug/quantumd`) per `AGENTS.md`.
2. Open the files panel. Type a few letters over a pane → first match highlights; press Enter → enters it.
3. Click the last row, type a letter matching a different entry, Enter → opens the highlighted one, not nothing.
4. Click into the toolbar filter box, type, press Enter → enters the highlighted folder.
5. Escape clears the filter and the selection.
