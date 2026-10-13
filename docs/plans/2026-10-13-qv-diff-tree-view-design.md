# qv diff mode: folder tree view for the Changes sidebar

**Status:** design approved by Ross 2026-10-13; implemented the same day (`96b725fa`..`dadc94f8`),
awaiting the manual walkthrough on x1 after the next `just switch`. Additions beyond this design, from
code review: a staging queue and live target resolution for folder staging, checkboxes no longer count
as text fields for shortcuts, `t` in the key hints, and a code-unit tie-break after `localeCompare`
(see section 1).
**Decided in:** `docs/playgrounds/2026-10-13-qv-diff-tree-view.html`; "Settled choices" restates Ross's
settled prompt from it. Builds on `docs/plans/2026-10-08-qv-diff-mode-design.md`.

## Goal

In `qv --diff` and `qv --diff <ref>` (the git forms; `qv <a> <b>` has no sidebar), the Changes sidebar shows the changed files as
a folder tree by default, switchable back to today's flat list. Requested while reviewing a 13-file
murmur8 change whose deep .NET paths made the flat list hard to scan.

## Settled choices

From the playground, verbatim in substance:

| Question | Choice |
|----------|--------|
| Starting view | Tree, every time a window opens (not remembered across windows) |
| Toggle | Key `t`, plus an icon button in the sidebar title |
| Unstaged / Staged | Kept; each group holds its own tree; a partly staged file appears in both, as today |
| Single-child folder chains | Compacted into one row (`src/Murmur8.Application/AI`) |
| Order inside a folder | Folders and files mixed, alphabetical |
| Main pane order | Follows the sidebar, so `]` and `[` walk the tree |
| Folder row | Chevron and name only; click folds and unfolds |
| Folder staging | A folder checkbox stages or unstages every file under it |
| File row in tree mode | One line: status, name, partly-staged dot, `+N −M`; no directory line (a renamed file keeps its "renamed from" line) |
| Indent | 16px per level, no guide lines |
| Sidebar width | 260px (from 220px), in both views |

Not remembered across windows: qv persists no view settings today (Unified/Split also resets), and
remembering would need a new daemon-side preferences store. Ross chose "always open as tree".

**One consequence to know:** with a tree per section, every file under an Unstaged folder is
unstaged and every file under a Staged folder is staged. So a folder checkbox is never "mixed": in
the Unstaged tree it is empty and stages everything under it; in the Staged tree it is ticked and
unstages everything under it.

## Architecture

All frontend, in `src/ui/plugins/file-viewer/views/file-viewer/src/lib/diff/`. No Rust, IPC or
client-type change: folder staging reuses `file-viewer.stage` / `file-viewer.unstage` per file.

### 1. `changeTree.ts` (new, pure, no Svelte)

- `compareTreePaths(left: string, right: string): number`: compares paths segment by segment
  (split on `/`) with `localeCompare`, falling back to code-unit order when `localeCompare` calls two
  different names equal (for example NFC and NFD accents); the first differing segment decides, and a
  file sorts before a same-named folder. This is exactly the depth-first order of a mixed-alphabetical
  tree.
- `buildChangeTree(entries: ReviewEntry[]): FolderNode`: groups one section's entries into folders,
  then compacts every chain of folders that has no files and a single child folder into one node
  whose `name` is the joined segments (`Murmur8.Application/Calendar`) and whose `path` is the full
  path of the last folder. A compacted folder sorts by its first segment.
- `flattenChangeTree(root, collapsed: Set<string>): TreeRow[]`, the task-manager `tree.ts`
  pattern: `TreeRow = { kind: 'folder', depth, folder, expanded } | { kind: 'file', depth, entry }`.
  A collapsed folder emits its row but not its descendants.
- `entriesUnder(folder): ReviewEntry[]`: every entry below a folder, in tree order.

Acceptance criteria:
- Sorting entries with `compareTreePaths` gives the same order as reading the file rows of
  `flattenChangeTree` with nothing collapsed (tested on the 13-file murmur8 set and on edge cases:
  root-level files, `a.ts` beside folder `a`, a chain ending in files, case differences).
- `.github/workflows/runner-watchdog.yml` alone yields one folder row `.github/workflows` and one
  file row.
- No function mutates its input.

### 2. `DiffView.svelte`: view state and pane order

- New state `sidebarView: 'tree' | 'list' = 'tree'`.
- `entries` becomes: `reviewEntries(changeSet)`, then, when `sidebarView === 'tree'`, re-sorted by
  section (Unstaged before Staged, unchanged) and `compareTreePaths`. The main pane, `activeIndex`,
  `]`/`[`, `n`/`p` and scroll tracking all keep using `entries`, so they follow the tree order with
  no further change. List mode keeps today's order exactly.
- Switching view keeps the same file active: look up the active entry's `id` in the new order and
  scroll to it.
- New `handleStageMany(entries)` / `handleUnstageMany(entries)`: call the existing per-file IPC
  sequentially in tree order and stop at the first failure, showing it in the existing staging
  error banner; files already staged stay staged. A staged rename still triggers `refresh()`.
- Width: `.changes-sidebar` 260px.

Acceptance criteria:
- A fresh window opens in tree view; `t` switches to list and back; `t` does nothing while
  typing in the search field (same `inField` rule as `s`).
- In tree view the pane's file sections appear in tree order; in list view, in today's order.
- After `t`, the previously active file is still active and in view.
- Staging a folder of three files sends three `file-viewer.stage` calls, in tree order; if the
  second fails, the third is not sent and the banner shows the error.

### 3. `ChangesSidebar.svelte`: rendering

- New props: `view: 'tree' | 'list'`, `onToggleView()`, `onStageMany(entries)`,
  `onUnstageMany(entries)`.
- Title: `Changes`, file count, then a 22×20px toggle button (`aria-pressed` on tree,
  `title="Show as list (t)"` / `"Show as tree (t)"`), using a Unicode glyph, never emoji.
- List view: exactly today's markup.
- Tree view, per group: the group heading as today, then the rows of `flattenChangeTree`.
  - Folder row: checkbox (when stageable), 16px of indent per depth, chevron, name ending in `/`,
    truncated from the left so the deepest part stays visible (`direction: rtl` with the text
    inside `<bdi>`; without it the trailing `/` jumps to the front).
  - File row: checkbox, indent, status, name, partly-staged dot, `+N −M` on one line.
  - Folded state is per group and per folder path (`unstaged|src/Murmur8.Web`), local to the
    window, all folders open at start.
  - When the active file is inside a folded folder, that folder row takes the active style.

Acceptance criteria:
- With the murmur8 set, the Unstaged tree shows `src/` then `Murmur8.Application/Calendar/`
  (compacted) with no directory line on file rows.
- Clicking a folder row folds it and does not change the active file; the pane is unaffected.
- A folder checkbox calls `onStageMany` (Unstaged tree) or `onUnstageMany` (Staged tree) with
  every entry below it, and does not select anything.
- A non-stageable git change set (`qv --diff <ref>`) shows the tree without checkboxes.

## Error handling

Folder staging can fail midway (index lock, a file changed on disk). The sequential loop stops,
the existing banner shows the message, and the sidebar shows the true state, because each file's
success already updated the local change set. No rollback: a half-staged folder is the honest
result, and the dot and counts already describe it.

## Testing

vitest + jsdom, beside the existing diff tests:
- `changeTree.test.ts` (new): ordering, compaction, flatten with folding, `entriesUnder`, the
  comparator-matches-flatten property on the murmur8 fixture and the edge cases above.
- `ChangesSidebar.test.ts`: tree is rendered when `view = 'tree'`, list markup unchanged when
  `view = 'list'`, toggle button calls `onToggleView`, folder fold, folder checkbox, one-line rows.
- `DiffView.test.ts`: opens in tree, `t` toggles, pane order follows the view, active file kept
  across a toggle, `handleStageMany` order and stop-on-failure.
- `viewerKeymap` test for `t`.

Manual check: build the view, run a dev daemon, `qv --diff` in murmur8 with a mixed staged state;
Ross walks it one case at a time.

## Out of scope

- Remembering tree or list across windows.
- Folder-level unified diff, folder search or path filter.
- Changing the flat list's order or look.
- Per-hunk staging.

## Assumptions

- Paths in a git change set are repository-relative with `/` separators (git porcelain output).
- The sidebar stays git-only (`DiffView.svelte` renders it only for `source.kind === 'git'`), so
  pair mode is unaffected; its `entries` order stays as today regardless of `sidebarView`.
- `localeCompare` with no explicit locale, the same as today's sort.
