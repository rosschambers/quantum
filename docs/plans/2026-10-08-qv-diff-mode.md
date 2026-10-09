# qv Diff Mode, Search Performance, and Scrollbar Markers — Implementation Plan

## Execution status (2026-10-09)

**Complete, live on x1, and accepted by Ross** in a guided walkthrough covering the overview,
staging with the changed-on-disk banner, Ctrl+F on a large Markdown file, and copy with split
view. Final pins: quantum `e97f9dcc`, config `6e941bd`. The tasks below are the original plan;
where the built code differs, this section wins.

Deviations and additions found in review or live use:

- **Security:** working-tree reads and hashing never follow symlinks (git's model: the content is
  the link target). Staging validates `mode` (100644, 100755, or 120000) and requires the exact
  repository toplevel. Oversized committed blobs and submodule (gitlink) entries are skipped
  before they are read.
- **Unstage takes `old_path`** so that unstaging a staged rename restores both paths. The
  frontend refetches after this one case. A `git rm --cached` file is reported as ONE changed
  file, not two.
- **The fingerprint** hashes the sorted changed-path set plus working-file metadata, never the
  status letters, so qv's own staging does not trigger the banner. The banner text carries no
  count.
- **Ruler marks are measured** against the scroll content (rows carry `data-old-index`,
  `data-new-index`, `data-first-new-index`, and `data-hidden-count`), not computed as equal
  per-file shares. Clicking a change mark jumps to that change.
- **Split view** uses one pair of columns per file, so horizontal scroll stays in sync across hunks.
- **Live-use fixes:** client bridge reply routing for several clients on one page (`5c2ea9bb`; the
  "Loading changes" hang); a final newline no longer counts as an extra line; the header reads
  "N staged · M partly"; read errors show the real message instead of "[object Object]"; the
  sidebar's Unstaged and Staged groups collapse.

> **For OpenCode:** REQUIRED SUB-SKILL: Use superpowers:executing-plans (or subagent-driven-development) to implement this plan task-by-task.

**Goal:** Add a git-aware, syntax-highlighted diff mode with file staging to qv, fix the quadratic Ctrl+F slowdown, add search-match markers on a shared overview ruler, and fix three small bugs found along the way.

**Architecture:** Rust supplies file contents and runs git through a new `quantum-git` infrastructure crate behind a domain `RepositoryReview` port, exposed as four `file-viewer.*` IPC methods. The frontend diffs full old/new contents (the `diff` package), highlights whole files with highlight.js, and renders through one `DiffView`. A shared `OverviewRuler` serves both search markers (every renderer) and diff change marks.

**Tech Stack:** Rust 2021 (tokio, serde, thiserror, async-trait), git CLI 2.55, Svelte 5 runes, TypeScript, highlight.js 11, `diff` 9 (bundled types), vitest 2 + jsdom + @testing-library/svelte.

**Design (read first):** `docs/plans/2026-10-08-qv-diff-mode-design.md` — the settled spec, decisions D1/D2, and the "Folded in" research. **Visual reference:** `docs/playgrounds/qv-diff-playground.html` (working, verified prototype logic: `lineDiff`, `intraLine`, `buildRows`, `renderCode`, `selectedCodeText`, `reviewEntries`, `layoutStage`/ruler). Where a task says "port from the playground", translate that function to typed TypeScript faithfully, then make it pass the task's tests.

**Assumptions:**
- Work happens directly on `main` in the canonical checkout (Ross's standing preference). **The checkout contains another session's uncommitted work** (power-menu, sound-menu, `src/ui/packages/client/src/shown.ts`, host window files). Never `git add -A` or `git commit -a`; always stage explicit paths.
- git ≥ 2.23 on every host (`git restore`); this host has 2.55.
- `diff` v9 ships TypeScript types (`npm view diff types` → `libcjs/index.d.ts`); no `@types/diff`.
- The architecture test classifies crates by path (`tests/architecture-test/src/lib.rs:59-100`), so a new `src/infrastructure/git` crate needs no test change — only the workspace member and the `AGENTS.md` table.
- WebKitGTK supports `color-mix()` (already used at `Header.svelte:72`).

## Conventions for every task

| Need | Command (run from the quantum repo root) |
|---|---|
| One frontend test file | `./scripts/devsh.sh bash -c "cd src/ui/plugins/file-viewer/views/file-viewer && pnpm exec vitest run <pattern>"` |
| All file-viewer tests | `./scripts/devsh.sh bash -c "cd src/ui/plugins/file-viewer/views/file-viewer && pnpm test"` |
| Client package tests | `./scripts/devsh.sh bash -c "cd src/ui && pnpm --filter @quantum/client test"` |
| One Rust crate | `./scripts/devsh.sh cargo test -p <crate>` |
| Format / lint | `./scripts/devsh.sh cargo fmt --all` then `just lint` (the justfile recipes call devsh themselves) |
| Frontend build | `just frontend-build` (runs `pnpm -C src/ui -r build` through devsh; topological — never add `--parallel`) |

- Set tool timeouts to at least 10 minutes for any devsh command (first nix-shell entry and cargo builds are slow).
- TDD: write the failing test, run it and see the expected failure, implement, see it pass, commit.
- Svelte components under test use `$effect`, never `onMount` (AGENTS.md). Stub `ResizeObserver` and `scrollIntoView` like `src/App.test.ts:17-28`. Mock the client like `src/App.test.ts:4-8`.
- No abbreviations in identifiers, comments, or commit messages. No emojis. No `unwrap`/`expect` outside tests and `main`.
- Commit per task with conventional messages, staging only the task's files.
- **Never open GUI windows** (no `qv`, no browser) during execution; Task 34 is the single, asked-for look.

**Paths used below:** `VIEW=src/ui/plugins/file-viewer/views/file-viewer` and `LIB=$VIEW/src/lib`.

---

## Phase 0 — Baseline

### Task 0: Record the baseline

**Acceptance Criteria:**
- [ ] File-viewer vitest suite result recorded (pass count, any pre-existing failures named).
- [ ] `cargo test -p quantum-domain -p quantum-application` result recorded.
- [ ] No files changed.

**Step 1:** Run the file-viewer suite and the two Rust crates; write the counts into your working notes. Any pre-existing failure is not yours to fix — note it and continue.

---

## Phase 1 — Ctrl+F performance (root cause: design doc "Folded in")

### Task 1: Linear match-segment search without Range objects

**Files:**
- Modify: `$LIB/search.ts`
- Test: `$LIB/search.test.ts`

**Acceptance Criteria:**
- [ ] New export `findMatchSegmentsInDom(root: Element, query: string): MatchSegments` where `MatchSegments = { count: number; segments: MatchSegment[] }` and `MatchSegment = { node: Text; start: number; end: number; matchIndex: number }`.
- [ ] Segments are produced by one ordered two-pointer walk over `spans` and `findMatches` results: O(spans + matches); no `Range` is created; no `Array.prototype.find` per match.
- [ ] Segments come in document order; a match crossing an inline boundary yields one segment per text node, all sharing its `matchIndex`.
- [ ] `count` equals `findMatchesInDom(root, query).length` for every test fixture (parity).
- [ ] Diagram subtrees (`.diagram-block`, `.diagram-error`) produce no segments.
- [ ] Separator newlines inserted at block boundaries never produce zero-length segments.
- [ ] `findMatchesInDom` is kept (other callers/tests) but is no longer used by `MarkdownRenderer` after Task 2.

**Step 1: Write the failing tests** (append to `search.test.ts`; extend its existing import to `import { findMatches, findMatchesInLines, findMatchesInDom, findMatchSegmentsInDom } from './search';` — the file uses `test`, not `it`):

```ts

function domFrom(html: string): HTMLElement {
    const root = document.createElement('div');
    root.innerHTML = html;
    return root;
}

describe('findMatchSegmentsInDom', () => {
    test('returns one segment per text node a match covers, sharing the match index', () => {
        const root = domFrom('<p>nee<strong>dle</strong> and needle</p>');
        const result = findMatchSegmentsInDom(root, 'needle');
        expect(result.count).toBe(2);
        expect(result.segments.map((segment) => [segment.node.data.slice(segment.start, segment.end), segment.matchIndex]))
            .toEqual([['nee', 0], ['dle', 0], ['needle', 1]]);
    });

    test('never matches across a block boundary and skips diagram source', () => {
        const root = domFrom('<p>nee</p><p>dle</p><div class="diagram-block">needle</div>');
        expect(findMatchSegmentsInDom(root, 'needle').count).toBe(0);
    });

    test('has the same count as findMatchesInDom', () => {
        const root = domFrom('<h1>Upload</h1><p>up <em>upl</em>oad upload</p><ul><li>UPLOAD</li></ul>');
        for (const query of ['u', 'up', 'upload', 'oad u']) {
            expect(findMatchSegmentsInDom(root, query).count).toBe(findMatchesInDom(root, query).length);
        }
    });

    test('stays linear: 300 paragraphs with 150 matches finish quickly', () => {
        const html = Array.from({ length: 300 }, (_, index) => `<p>line ${index}${index % 2 === 0 ? ' token' : ''} <em>tail</em></p>`).join('');
        const root = domFrom(html);
        const started = performance.now();
        const result = findMatchSegmentsInDom(root, 'token');
        expect(result.count).toBe(150);
        expect(performance.now() - started).toBeLessThan(500);
    });
});
```

**Step 2:** Run `search.test.ts`. Expected: FAIL, `findMatchSegmentsInDom` is not exported.

**Step 3: Implement** in `search.ts` (reuses the private `collectTextSpans`):

```ts
export interface MatchSegment {
    node: Text;
    start: number;
    end: number;
    matchIndex: number;
}

export interface MatchSegments {
    count: number;
    segments: MatchSegment[];
}

/**
 * Like findMatchesInDom, but returns per-text-node segments computed from the
 * offsets collectTextSpans already produced, with one ordered two-pointer walk.
 * Creates no Range objects: WebKit keeps live ranges updated on every DOM
 * mutation, which made wrapping quadratic in practice (design doc, "Folded in").
 */
export function findMatchSegmentsInDom(root: Element, query: string): MatchSegments {
    if (!query) {
        return { count: 0, segments: [] };
    }
    const { text, spans } = collectTextSpans(root);
    const matches = findMatches(text, query);
    const segments: MatchSegment[] = [];
    let spanIndex = 0;
    matches.forEach((match, matchIndex) => {
        while (spanIndex < spans.length && spans[spanIndex].end <= match.start) {
            spanIndex++;
        }
        for (let cursor = spanIndex; cursor < spans.length && spans[cursor].start < match.end; cursor++) {
            const span = spans[cursor];
            const start = Math.max(match.start, span.start) - span.start;
            const end = Math.min(match.end, span.end) - span.start;
            if (end > start) {
                segments.push({ node: span.node, start, end, matchIndex });
            }
        }
    });
    return { count: matches.length, segments };
}
```

**Step 4:** Run `search.test.ts`. Expected: PASS (all old and new tests).

**Step 5: Commit**
```bash
git add $LIB/search.ts $LIB/search.test.ts
git commit -m "perf: compute markdown search segments in one linear pass"
```

### Task 2: Markdown renderer wraps segments with splitText

**Files:**
- Modify: `$LIB/MarkdownRenderer.svelte:225-281` (`wrapRanges`, the search `$effect`)
- Test: `$LIB/MarkdownRenderer.test.ts`

**Acceptance Criteria:**
- [ ] The search `$effect` calls `findMatchSegmentsInDom` once; `wrapRanges` is replaced by `wrapSegments(segments, count): HTMLElement[][]` that processes segments right to left, isolating each segment with `Text.splitText` and wrapping it in `<mark class="search-match">`. No `document.createRange`, no `intersectsNode`.
- [ ] `matchMarks` keeps its shape (`HTMLElement[][]`, one inner array per match, document order) so the current-match `$effect` (`:283-289`) is unchanged.
- [ ] `onMatchCount` receives `count`.
- [ ] Existing MarkdownRenderer tests pass unchanged (element preservation, cross-inline match, diagram never wrapped).
- [ ] New regression test: a 300-paragraph document with 150 matches completes the search flush in under 1000 ms in jsdom (it took over 16 s before).

**Step 1: Write the failing test** (append to `MarkdownRenderer.test.ts`; add `import { tick } from 'svelte';` — the file already uses `render`/`rerender` from `@testing-library/svelte/svelte5`):

```ts
it('wraps many matches without a quadratic node scan', async () => {
    const content = Array.from({ length: 300 }, (_, index) => `Paragraph ${index}${index % 2 === 0 ? ' token' : ''} *tail*`).join('\n\n');
    const counts: number[] = [];
    const { rerender } = render(MarkdownRenderer, { content, query: '', onMatchCount: (count: number) => counts.push(count) });
    const started = performance.now();
    await rerender({ content, query: 'token', onMatchCount: (count: number) => counts.push(count) });
    await tick();
    expect(performance.now() - started).toBeLessThan(1000);
    expect(counts.at(-1)).toBe(150);
    expect(document.querySelectorAll('mark.search-match')).toHaveLength(150);
});
```

**Step 2:** Run it. Expected: FAIL on the time budget (or a test timeout).

**Step 3: Implement.** Replace `wrapRanges` and the search effect:

```ts
function wrapSegments(segments: MatchSegment[], count: number): HTMLElement[][] {
    const marks: HTMLElement[][] = Array.from({ length: count }, () => []);
    // Right to left, so earlier offsets in the same text node stay valid.
    for (let index = segments.length - 1; index >= 0; index--) {
        const { node, start, end, matchIndex } = segments[index];
        const tail = node.splitText(start);
        tail.splitText(end - start);
        const mark = document.createElement('mark');
        mark.className = 'search-match';
        tail.parentNode?.insertBefore(mark, tail);
        mark.appendChild(tail);
        marks[matchIndex].unshift(mark);
    }
    return marks;
}

$effect(() => {
    void parsedHtml;
    if (!container) return;
    const { count, segments } = findMatchSegmentsInDom(container, query);
    const marks = wrapSegments(segments, count);
    matchMarks = marks;
    onMatchCount?.(count);
    return () => clearMarks(marks);
});
```

Update the import to `import { findMatchSegmentsInDom, type MatchSegment } from './search';`. Note: splitting the same node twice right-to-left is safe because a later segment in the same node has a larger `start`, and `splitText(start)` on the original node leaves the earlier text in place.

**Step 4:** Run the whole MarkdownRenderer test file and `App.test.ts`. Expected: PASS.

**Step 5: Commit**
```bash
git add $LIB/MarkdownRenderer.svelte $LIB/MarkdownRenderer.test.ts
git commit -m "perf: wrap markdown search matches with splitText, no live ranges"
```

### Task 3: Code and JSON renderers stop re-highlighting every line per keystroke

**Files:**
- Modify: `$LIB/CodeRenderer.svelte:74-84`, `$LIB/JsonFoldRenderer.svelte:169-177` (and its use at `:252`)
- Test: `$LIB/CodeRenderer.test.ts`, `$LIB/JsonFoldRenderer.test.ts`

**Acceptance Criteria:**
- [ ] A `baseHighlightedLines = $derived(lines.map((line) => highlightCode(line, language)))` depends only on `lines`/`language`.
- [ ] The per-line HTML used for rendering is the base string for lines without matches and `highlightLineWithMatches(...)` only for lines with matches.
- [ ] Collapsed fold header rendering (`CodeRenderer.svelte:136-147`) keeps current behavior.
- [ ] Test: with `highlightCode` spied (`vi.spyOn` on the `./highlighter` module namespace, or `vi.mock` with a passthrough), changing the query on a 400-line file whose query matches 2 lines calls `highlightCode` for at most a small constant times the matched-line segment count — not 400 times.

**Step 1:** Write the spy test for CodeRenderer (and an equivalent for JsonFoldRenderer using its line model). Mount with `query: ''`, reset the spy, rerender with a query matching exactly 2 lines, assert `spy.mock.calls.length < 20`.

**Step 2:** Run. Expected: FAIL (about 400 calls).

**Step 3:** Implement the split described in the criteria. In CodeRenderer, `highlightedLineHtml(index, text)` becomes: `matchesForLine(index).length === 0 ? baseHighlightedLines[index] : highlightLineWithMatches(text, language, matches, currentRangeForLine(index))`. Keep the virtual-scrolling snippet calling `highlightedLineHtml` (it now hits the cache for unmatched lines).

**Step 4:** Run both renderer test files and `App.test.ts`. Expected: PASS.

**Step 5: Commit**
```bash
git add $LIB/CodeRenderer.svelte $LIB/CodeRenderer.test.ts $LIB/JsonFoldRenderer.svelte $LIB/JsonFoldRenderer.test.ts
git commit -m "perf: cache syntax highlighting per line across search keystrokes"
```

---

## Phase 2 — Bugs found during research

### Task 4: Default theme defines --color-error

**Files:**
- Modify: `src/ui/themes/default/tokens.toml` (the `[colors]` table), `AGENTS.md` (overlay house-style token list)

**Acceptance Criteria:**
- [ ] `default/tokens.toml` gains `color-error = "#e5484d"` (distinct from its `color-warning = "#f38ba8"`).
- [ ] `AGENTS.md` keeps listing `--color-error` as theme-backed (now true for both themes).
- [ ] `touch src/infrastructure/theme-store/src/store.rs` noted in the commit body (embedded themes need it to rebuild).
- [ ] `cargo test -p quantum-theme` passes.

**Step 1:** If `quantum-theme` has a test asserting the token set of `default`, extend it to require `color-error`; otherwise add one in the theme-store tests: load the embedded `default` theme and assert `color-error` is present. Run, see it fail.
**Step 2:** Add the token. Run the test, see it pass.
**Step 3: Commit**
```bash
git add src/ui/themes/default/tokens.toml src/infrastructure/theme-store/src AGENTS.md
git commit -m "fix: define color-error in the default theme"
```

### Task 5: Short text files scroll

**Files:**
- Modify: `$LIB/TextRenderer.svelte:70-95`
- Test: `$LIB/TextRenderer.test.ts`

**Acceptance Criteria:**
- [ ] Both non-virtual branches render inside one `<div class="text-scroller">` with `height: 100%; overflow: auto`.
- [ ] The scroller is exposed as a bindable `scrollElement` prop (used by Task 11).
- [ ] Test: the non-virtual render contains an element with class `text-scroller` wrapping the `<pre>`; its computed `overflow` (inline style or class rule checked via `getComputedStyle`) is `auto`.

Steps: failing test → wrap both branches → pass → commit `fix: make short text files scrollable in the file viewer`.

### Task 6: Code line numbers scroll with the code (short files)

**Files:**
- Modify: `$LIB/CodeRenderer.svelte:187-213` and its styles `:220-275`
- Test: `$LIB/CodeRenderer.test.ts`

**Acceptance Criteria:**
- [ ] In the non-virtual path, one element (`.code-scroller`, `overflow: auto`, `height: 100%`) contains both the gutter and the code; the gutter is `position: sticky; left: 0` with `background: var(--color-bg)` so it stays visible when scrolling sideways and moves with vertical scroll — the `JsonFoldRenderer` pattern (`JsonFoldRenderer.svelte:224-287`).
- [ ] `.code-renderer` no longer clips the gutter separately.
- [ ] The scroller is exposed as bindable `scrollElement` (used by Task 11).
- [ ] Test: gutter and code share the same scroll container (`gutter.closest('.code-scroller') === code.closest('.code-scroller')`).
- [ ] Existing fold and search tests pass.

Steps: failing test → restructure markup/CSS → pass → commit `fix: scroll code line numbers together with the code`.

---

## Phase 3 — Shared overview ruler and search markers

Spec: design doc "Scrollbar match markers". Geometry constants: virtual code rows 21px with 12px padding (`CodeRenderer.svelte:170-174`); virtual text rows 21px with 32px padding (`TextRenderer.svelte:71`); non-virtual code/JSON rows 20.8px (13px × 1.6) with 12px padding.

### Task 7: Pure ruler math

**Files:**
- Create: `$LIB/overviewRuler.ts`
- Test: `$LIB/overviewRuler.test.ts`

**Acceptance Criteria:**
- [ ] Exports exactly:
  ```ts
  export type RulerMarkKind = 'match' | 'current-match' | 'added' | 'removed' | 'mixed';
  export interface RulerMark { start: number; extent: number; kind: RulerMarkKind; index?: number }
  export interface RulerSegment { top: number; height: number; kind: RulerMarkKind; lane: 'full' | 'change' | 'search'; index?: number }
  export interface ScrollView { scrollTop: number; clientHeight: number; scrollHeight: number }
  export const MINIMUM_HEIGHT: Record<RulerMarkKind, number>; // match 2, current-match 4, added 2, removed 2, mixed 2
  export function layoutRulerSegments(marks: readonly RulerMark[], trackHeight: number): RulerSegment[];
  export function thumbGeometry(view: ScrollView, trackHeight: number): { top: number; height: number };
  export function scrollTopForTrackPosition(y: number, trackHeight: number, view: ScrollView): number;
  export function hitTestMark(marks: readonly RulerMark[], y: number, trackHeight: number, tolerance: number): RulerMark | null;
  export function searchMarks(positions: Float64Array, count: number, current: number | null): RulerMark[];
  export function rowCenterFraction(row: number, rowCount: number, geometry: { paddingTop: number; rowHeight: number; paddingBottom: number }): number;
  ```
- [ ] `layoutRulerSegments`: drops non-finite marks; clamps `start` to [0, 1]; pixel height `max(extent × trackHeight, MINIMUM_HEIGHT[kind])`; keeps a mark at 1 inside the track (`top ≤ trackHeight − height`); merges touching/overlapping marks of the same kind and lane; never merges `current-match`, which is emitted last; lane is `'full'` for every mark when no change kinds exist, otherwise change kinds → `'change'`, `match` → `'search'`, `current-match` → `'full'`.
- [ ] `thumbGeometry`: `top = scrollTop / scrollHeight × trackHeight`, `height = max(8, clientHeight / scrollHeight × trackHeight)`, full track when content fits, clamped so `top + height ≤ trackHeight`.
- [ ] `scrollTopForTrackPosition`: centers (`y / trackHeight × scrollHeight − clientHeight / 2`), clamped to `[0, scrollHeight − clientHeight]`.
- [ ] `hitTestMark`: nearest mark (by pixel center) within `tolerance`, else `null`.
- [ ] `searchMarks`: `[]` when `positions.length !== count`; kind `current-match` at `current`, else `match`; `index` = match index; `extent` 0.
- [ ] `rowCenterFraction`: `(paddingTop + row × rowHeight + rowHeight / 2) / (paddingTop + rowCount × rowHeight + paddingBottom)`.
- [ ] Tests 1-13 from the design research (listed below) pass.

**Step 1: Write the failing tests** — one `it` each:
1. point mark placed at `start × trackHeight` with its kind's minimum height;
2. a mark at 1 stays inside the track;
3. `NaN`/`Infinity` dropped, `-0.2`/`1.4` clamped;
4. same-kind touching marks merge, different kinds do not;
5. current match never merges and is last;
6. extent mark height = `max(extent × track, minimum)`;
7. lanes `'full'` with search-only marks, split with change marks present;
8. 10,000 random match marks on a 600px track produce at most 300 search-lane segments;
9. `thumbGeometry` proportional / full when content fits / minimum 8 / clamped;
10. `scrollTopForTrackPosition` centers and clamps at both ends;
11. `hitTestMark` nearest within tolerance, `null` outside;
12. `searchMarks` length mismatch → `[]`, current flagged, indexes kept;
13. `rowCenterFraction` for padding 12/12 rowHeight 21 and padding 32/32.

**Step 2:** Run; expect FAIL (module missing).
**Step 3:** Implement. Sort marks by `start`, convert to pixels, sweep per lane merging while `next.top <= current.top + current.height`.
**Step 4:** Run; PASS.
**Step 5:** `git add $LIB/overviewRuler.ts $LIB/overviewRuler.test.ts && git commit -m "feat: add overview ruler layout math"`

### Task 8: Visible row mapping for folds

**Files:**
- Modify: `$LIB/fold-model.ts`
- Test: `$LIB/fold-model.test.ts`

**Acceptance Criteria:**
- [ ] `export function visibleRowOfLine(lineCount: number, model: Map<number, CodeFoldRange>, collapsed: (startLine: number) => boolean): { rowOfLine: Int32Array; rowCount: number }`.
- [ ] Walk identical to `CodeRenderer.svelte:129-159`: a collapsed fold contributes one row (its header) and every hidden line maps to the header's row; nested collapsed folds map to the outermost collapsed header.
- [ ] Tests: identity with no folds; collapsed fold maps hidden lines to the header and shifts later rows; nested → outermost; expanded folds hide nothing.

Steps: tests → fail → implement → pass → `git commit -m "feat: map source lines to visible rows across folds"`.

### Task 9: Batched DOM measurement helper

**Files:**
- Create: `$LIB/measureOffsets.ts`
- Test: `$LIB/measureOffsets.test.ts`

**Acceptance Criteria:**
- [ ] `measureFractions(root: HTMLElement, anchors: ArrayLike<Element | Range>): Float64Array` reads `root.getBoundingClientRect()` once, then each anchor's rect; returns `(anchorTop − rootTop) / rootHeight`; a zero-size anchor element falls back to its nearest ancestor with a non-zero rect; returns an empty array when `rootHeight` is 0; performs no DOM writes.
- [ ] `observeLayout(root: HTMLElement, onChange: () => void): () => void` — one `ResizeObserver` on `root`; callbacks coalesced into at most one `onChange` per animation frame; the returned function disconnects and cancels a pending frame.
- [ ] Tests 18-22 from the research: root rect read once (spy); zero-size fallback; zero-height root → empty, no `NaN`; no mutations (`MutationObserver` records none); several observer callbacks in one frame → one `onChange`.

Steps: tests (stub `getBoundingClientRect` and `ResizeObserver`) → fail → implement → pass → `git commit -m "feat: add batched offset measurement for ruler marks"`.

### Task 10: OverviewRuler component

**Files:**
- Create: `$LIB/OverviewRuler.svelte`
- Test: `$LIB/OverviewRuler.test.ts`

**Acceptance Criteria:**
- [ ] Props: `marks: readonly RulerMark[]`, `scrollElement: HTMLElement | null | undefined`, `onMarkActivate?: (mark: RulerMark) => void`, `hitTolerance = 4`.
- [ ] Root `<div class="overview-ruler" aria-hidden="true">`, width 10px, transparent; `border-left: 1px solid var(--color-border)` and the thumb render **only when `marks.length > 0`**.
- [ ] Renders one `<div class="ruler-mark kind-<kind>">` per `layoutRulerSegments` segment with pixel `top`/`height` and lane classes (`lane-full` spans the inner 7px; `lane-change` left 3px; `lane-search` right 3px).
- [ ] Colors (tokens only): match `color-mix(in oklab, var(--color-fg) 65%, transparent)`; current-match `var(--color-fg)` + `box-shadow: 0 0 0 1px var(--color-bg)`; added `var(--color-accent)`; removed `var(--color-error, #e5484d)`; mixed `linear-gradient(to right, var(--color-accent) 50%, var(--color-error, #e5484d) 50%)`; thumb `color-mix(in oklab, var(--color-fg) 12%, transparent)`, `pointer-events: none`, beneath marks.
- [ ] In `$effect`: passive `scroll` listener on `scrollElement` and a `ResizeObserver` on itself and `scrollElement`, thumb updates coalesced to one per frame; all removed on teardown.
- [ ] `pointerdown` calls `preventDefault()` (keeps focus in the search input). Click: `hitTestMark` hit → `onMarkActivate(mark)` and no scroll; miss → `scrollElement.scrollTop = scrollTopForTrackPosition(...)`.
- [ ] Tests 23-29 from the research pass.

Steps: tests → fail → implement → pass → `git commit -m "feat: add shared overview ruler component"`.

### Task 11: Renderers report match positions and expose their scroller

**Files:**
- Modify: `$LIB/VirtualScroller.svelte` (add bindable `container`), `$LIB/CodeRenderer.svelte`, `$LIB/JsonFoldRenderer.svelte`, `$LIB/TextRenderer.svelte`, `$LIB/MarkdownRenderer.svelte`
- Test: each renderer's test file

**Acceptance Criteria:**
- [ ] Every renderer accepts `onMatchPositions?: (positions: Float64Array) => void` and `scrollElement = $bindable()`.
- [ ] Positions are in `currentMatchIndex` order and their length equals the count passed to `onMatchCount` in the same update.
- [ ] Code virtual: `rowCenterFraction(lineIndex, lineCount, { paddingTop: 12, rowHeight: 21, paddingBottom: 12 })`. Code short and JSON: `rowCenterFraction(rowOfLine[lineIndex], rowCount, { paddingTop: 12, rowHeight: 20.8, paddingBottom: 12 })` using Task 8. Text virtual: padding 32. Text short (wraps) and Markdown: `measureFractions` over each match's first anchor (`[data-line]` span / `matchMarks[i][0]`), scheduled with `requestAnimationFrame` after each search pass and re-run via `observeLayout` on the content root and after the mermaid render loop completes (`MarkdownRenderer.svelte:196-217`).
- [ ] Changing only `currentMatchIndex` never triggers a measurement.
- [ ] Tests 30-34 from the research pass (virtual code arithmetic; collapsed fold reports header row then own row after expanding; JSON same; text padding 32; Markdown reports after a frame with stubbed rects and re-reports on a resize callback without calling `onMatchCount` again).

Steps: tests per renderer → fail → implement → pass → `git commit -m "feat: report search match positions from every renderer"`.

### Task 12: App shows the ruler and navigates from it

**Files:**
- Modify: `$VIEW/src/App.svelte`
- Test: `$VIEW/src/App.test.ts`

**Acceptance Criteria:**
- [ ] `let matchPositions = $state.raw(new Float64Array(0))`; `rulerMarks = $derived(searchMarks(matchPositions, totalMatches, currentMatchIndex))`.
- [ ] `.content-area` becomes a flex row: renderer (`flex: 1; min-width: 0`) + `OverviewRuler` (10px) for markdown, json, code, text; no ruler for image/video.
- [ ] `onMarkActivate` for a `match`/`current-match` mark sets `currentMatchIndex = mark.index` and increments `navigationRevision`.
- [ ] Closing search empties the marks (positions with a stale length are ignored by `searchMarks`).
- [ ] Tests 35-37: ruler column present/absent per file type; clicking a match mark updates "k of n" and calls `scrollIntoView`; closing search removes marks.

Steps: tests → fail → implement → pass → `git commit -m "feat: show search matches on the file viewer overview ruler"`.

---

## Phase 4 — Rust: repository review backend

### Task 13: Domain types, port, and the shared size cap

**Files:**
- Create: `src/domain/src/review.rs`
- Modify: `src/domain/src/lib.rs` (module + re-exports), `src/domain/src/ports.rs` (port), `src/domain/src/file_viewer.rs` (add `pub const VIEWER_TEXT_MAX_BYTES: u64 = 5 * 1024 * 1024;`), `src/infrastructure/files/src/filesystem.rs:446` (delete the private constant, import the domain one)

**Acceptance Criteria:**
- [ ] `review.rs` defines exactly the types below, serde `snake_case`, no new dependencies (domain allowlist: thiserror, serde, serde_json, async-trait, futures).
- [ ] `RepositoryReview` port is in `ports.rs` and re-exported from `lib.rs` with the types.
- [ ] `VIEWER_TEXT_MAX_BYTES` exists once (domain) and `quantum-files` uses it; `cargo test -p quantum-files` still passes.
- [ ] Unit tests: `ChangeSet` round-trips through `serde_json` with `snake_case` keys; a `FileSide` with `content: None` omits the key.

```rust
//! Repository review (qv diff mode) domain types.
use serde::{Deserialize, Serialize};

/// What to compare. `base` defaults to "HEAD"; `target: None` means the working tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct DiffSpec {
    pub repository: String,
    #[serde(default = "default_base")]
    pub base: String,
    #[serde(default)]
    pub target: Option<String>,
}

fn default_base() -> String {
    "HEAD".to_string()
}

/// One side of a changed file. `content` is absent for binary or oversized files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct FileSide {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    pub blob: String,
    pub mode: String,
    pub binary: bool,
    pub too_large: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ChangedFile {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// Absent when the file does not exist on that side (added / deleted / untracked).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base: Option<FileSide>,
    /// Present only when the change set is stageable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<FileSide>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<FileSide>,
    pub untracked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ChangeSet {
    pub repository_root: String,
    pub base_label: String,
    pub target_label: String,
    /// True only when base is HEAD and target is the working tree (decision D2).
    pub stageable: bool,
    pub files: Vec<ChangedFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "kind", content = "message", rename_all = "snake_case")]
pub enum ReviewError {
    #[error("not a git repository: {0}")]
    NotARepository(String),
    #[error("git is not available on quantumd's PATH: {0}")]
    GitUnavailable(String),
    #[error("git failed: {0}")]
    GitFailed(String),
    #[error("not stageable: {0}")]
    NotStageable(String),
    #[error("input/output error: {0}")]
    Io(String),
}
```

Port (in `ports.rs`, next to `FileSystemPort`):

```rust
/// Read a repository's changes and stage or unstage whole files (qv diff mode).
#[async_trait]
pub trait RepositoryReview: Send + Sync {
    async fn changes(&self, spec: &DiffSpec) -> Result<ChangeSet, ReviewError>;
    /// Stage exactly `blob` at `path` (decision D1). `blob: None` stages a deletion.
    async fn stage(&self, repository_root: &str, path: &str, blob: Option<&str>, mode: &str) -> Result<(), ReviewError>;
    async fn unstage(&self, repository_root: &str, path: &str) -> Result<(), ReviewError>;
    /// Cheap value that changes whenever the working tree or index changes.
    async fn fingerprint(&self, repository_root: &str) -> Result<String, ReviewError>;
}
```

Steps: write the serde tests → fail (module missing) → add types, port, constant move → `cargo test -p quantum-domain -p quantum-files` pass → `cargo fmt --all` → commit `feat: add repository review domain types and port`.

### Task 14: quantum-git crate, git runner, repository root

**Files:**
- Create: `src/infrastructure/git/Cargo.toml`, `src/infrastructure/git/src/lib.rs`, `src/infrastructure/git/src/runner.rs`
- Modify: root `Cargo.toml` (workspace member `src/infrastructure/git`), `AGENTS.md` (onion table row `src/infrastructure/git`; "seven sibling crates" sentence updated to the real count)

**Acceptance Criteria:**
- [ ] Crate `quantum-git` depends on `quantum-domain`, tokio, async-trait, tracing, serde, serde_json, thiserror; dev-dependency `tempfile = "3"`.
- [ ] `runner.rs`: `pub(crate) async fn run_git(root: &Path, arguments: &[&str], stdin: Option<&[u8]>) -> Result<Vec<u8>, ReviewError>` using `tokio::process::Command::new("git")` with `-C <root>`, env `GIT_OPTIONAL_LOCKS=0`, `LC_ALL=C`, no shell. Spawn error `NotFound` → `ReviewError::GitUnavailable(<error text>)`; non-zero exit → `ReviewError::GitFailed(<trimmed stderr>)`. Every failure is logged with `tracing::warn!` before returning (never swallowed).
- [ ] `pub async fn repository_root(directory: &str) -> Result<String, ReviewError>` runs `git rev-parse --show-toplevel` with `-C directory`; a "not a git repository" failure maps to `ReviewError::NotARepository(directory)`.
- [ ] Integration tests (temp directories): root of a fresh `git init` repository resolves (canonicalized paths compare equal); a subdirectory resolves to the same root; a non-repository directory yields `NotARepository`.
- [ ] Test helper `fn repository() -> (tempfile::TempDir, PathBuf)` that runs `git init -q`, sets `user.name`/`user.email` locally, and a helper `fn git(root, args)` for fixtures — shared by Tasks 15-19 in `tests/support/mod.rs`.
- [ ] `cargo test -p quantum-git` passes; the architecture test passes unchanged (`./scripts/devsh.sh cargo test -p quantum-architecture-test`).

Steps: tests → fail → implement → pass → fmt → commit `feat: add quantum-git crate with git runner`.

### Task 15: Parse git status (porcelain v2, NUL separated)

**Files:**
- Create: `src/infrastructure/git/src/status.rs`
- Test: unit tests in the same file

**Acceptance Criteria:**
- [ ] `pub(crate) fn parse_status(output: &[u8]) -> Vec<StatusEntry>` where
  ```rust
  pub(crate) struct StatusEntry {
      pub path: String,
      pub old_path: Option<String>,
      pub index_status: char,     // X: '.', 'M', 'A', 'D', 'R', ...
      pub worktree_status: char,  // Y
      pub head_mode: String, pub index_mode: String, pub worktree_mode: String,
      pub head_blob: String, pub index_blob: String,
      pub untracked: bool,
  }
  ```
- [ ] Record formats handled (input from `git status --porcelain=v2 -z --untracked-files=all`): `1 XY sub mH mI mW hH hI path`; `2 XY sub mH mI mW hH hI Xscore path` followed by a NUL-separated `origPath`; `? path` (untracked: modes/blobs empty, `untracked = true`); `u ...` (unmerged) and `! ...` (ignored) are skipped with a `tracing::warn!` for unmerged.
- [ ] Paths containing spaces survive (fields are split on the first 8 or 9 spaces only).
- [ ] Unit tests with literal byte strings for: modified unstaged (`.M`), staged (`M.`), partially staged (`MM`), added (`A.`), deleted unstaged (`.D`), renamed staged (`R.` with origPath), untracked, a path with spaces, unmerged skipped.

Steps: tests → fail → implement → pass → commit `feat: parse git porcelain v2 status`.

### Task 16: changes() for HEAD → working tree (stageable)

**Files:**
- Create: `src/infrastructure/git/src/review.rs` (`pub struct GitRepositoryReview;` implementing `RepositoryReview`), `src/infrastructure/git/src/contents.rs`
- Test: `src/infrastructure/git/tests/changes.rs`

**Acceptance Criteria:**
- [ ] For `DiffSpec { base: "HEAD", target: None }`: resolve root (Task 14), run status (Task 15), and build one `ChangedFile` per entry with `stageable = true`, `base_label = "HEAD"`, `target_label = "working tree"`.
- [ ] Sides: `base` from `head_blob` (absent when `head_mode` is `000000` or untracked), `index` from `index_blob` (absent when `index_mode` is `000000` or untracked), `target` read from disk at `root/path` (absent when deleted).
- [ ] Blob contents are fetched in **one** `git cat-file --batch` process per call (stdin = newline-separated blob ids; parse `<id> blob <size>\n<bytes>\n` records). Missing objects map to `GitFailed`.
- [ ] Working-tree side: size over `VIEWER_TEXT_MAX_BYTES` → `too_large: true, content: None`; `is_likely_binary` → `binary: true, content: None`; non-UTF-8 → treated as binary. Its `blob` comes from `git hash-object -w --path=<path> -- <file>` (writes the object so Stage can use it, decision D1); `mode` from the status `worktree_mode` (untracked: `100755` if the file is executable, else `100644`).
- [ ] Same binary/size rules for blob sides (size known from the batch header before reading content).
- [ ] `language` via `quantum_domain::language_for_extension` on the path's extension.
- [ ] Files are sorted by path. Empty repository changes → `files: []`.
- [ ] Integration tests build a repository with one commit, then: modify a file (unstaged), stage another, partially stage a third (stage, then edit), add an untracked file, add an ignored file (`.gitignore`) that must NOT appear, delete a file, rename a file with `git mv`, commit a binary file then change it, write a 6 MiB file. Assert each file's presence/absence, sides, `binary`/`too_large` flags, `old_path` for the rename, and that a working-tree blob id equals `git hash-object` of the file.

Steps: write the integration test → fail → implement → pass → commit `feat: read repository changes against HEAD`.

### Task 17: changes() for other bases and ref ranges (read-only)

**Files:**
- Modify: `src/infrastructure/git/src/review.rs`
- Test: `src/infrastructure/git/tests/changes.rs`

**Acceptance Criteria:**
- [ ] `base` other than `HEAD` with `target: None` → `git diff --raw -z -M <base>` plus `git ls-files --others --exclude-standard -z` for untracked; target read from disk; `stageable = false`; no `index` sides.
- [ ] `target: Some(ref)` (from `qv --diff A..B`) → `git diff --raw -z -M <base> <target>`; both sides from blobs; `stageable = false`; `target_label` = the ref.
- [ ] Parse `--raw -z` records: `:<srcmode> <dstmode> <srcsha> <dstsha> <status>[score]\0<src>\0[<dst>\0]` (a second path only for `R`/`C`); an all-zero `dstsha` means "read from the working tree".
- [ ] An unknown ref → `ReviewError::GitFailed` carrying git's stderr.
- [ ] Integration tests: `HEAD~1` → working tree on a two-commit repository (`stageable == false`, committed and uncommitted changes both present); `HEAD~1..HEAD` shows only the last commit; a bad ref errors.

Steps: tests → fail → implement → pass → commit `feat: read repository changes for arbitrary refs`.

### Task 18: Stage exactly what was shown; unstage

**Files:**
- Modify: `src/infrastructure/git/src/review.rs`
- Test: `src/infrastructure/git/tests/staging.rs`

**Acceptance Criteria:**
- [ ] `stage(root, path, Some(blob), mode)` runs `git update-index --add --cacheinfo <mode>,<blob>,<path>`; `stage(root, path, None, _)` runs `git update-index --force-remove -- <path>` (stages a deletion).
- [ ] `unstage(root, path)` runs `git restore --staged -- <path>`; for a path not in HEAD (staged new file) it runs `git rm --cached -q -- <path>` instead, so the file returns to untracked.
- [ ] `path` must be relative, must not contain a `..` component, and must not be absolute → otherwise `ReviewError::NotStageable`.
- [ ] Integration tests:
  - stage a modified file by the blob from `changes()`; `git diff --cached --name-only` lists it;
  - **D1 proof:** call `changes()`, then overwrite the file on disk, then stage the blob from the earlier `changes()`; `git show :<path>` equals the ORIGINAL content and `git diff --name-only` (unstaged) lists the file;
  - stage an untracked file → appears as added; unstage it → untracked again;
  - stage a deletion → `git diff --cached --name-status` shows `D`;
  - unstage a staged modification → back to unstaged;
  - `../escape` and `/etc/passwd` are rejected.

Steps: tests → fail → implement → pass → commit `feat: stage the exact reviewed blob and unstage files`.

### Task 19: Fingerprint

**Files:**
- Modify: `src/infrastructure/git/src/review.rs`
- Test: `src/infrastructure/git/tests/fingerprint.rs`

**Acceptance Criteria:**
- [ ] `fingerprint(root)` = hex of a stable 64-bit FNV-1a hash (implement inline; no new crate) over the **sorted set of paths** listed by `git status --porcelain=v2 -z --untracked-files=all` (use Task 15's parser), each followed by its working file's `(modified time in nanoseconds, size)` from `std::fs::metadata` (a missing file contributes a fixed marker). It deliberately does **not** hash the status letters: the banner means "changed on disk", and qv's own Stage/Unstage must not trigger it (staging changes the index columns, never the working files or the listed path set).
- [ ] Runs with `GIT_OPTIONAL_LOCKS=0` (the runner already sets it) so polling never takes `index.lock`.
- [ ] Integration tests: unchanged repository → equal fingerprints across two calls; re-editing an ALREADY-modified file → different; creating an untracked file → different; committing → different (paths leave the list); **staging a file → unchanged**.
- [ ] A code comment documents the accepted limitation: an external `git add` that changes no working file is not detected until the next manual refresh.

Steps: tests → fail → implement → pass → commit `feat: fingerprint repository state for change detection`.

### Task 20: ReviewService, IPC methods, and daemon wiring

**Files:**
- Create: `src/application/src/use_cases/review_service.rs`
- Modify: `src/application/src/use_cases/mod.rs`, `src/application/src/lib.rs` (export), `src/application/src/error.rs` (variant + codes), `src/application/src/dispatcher.rs` (field, constructor, four routes, handlers, test builder at `:1048-1110`), `src/binaries/quantumd/Cargo.toml` (`quantum-git` dependency), `src/binaries/quantumd/src/main.rs:1022-1100` (construct and pass)

**Acceptance Criteria:**
- [ ] `ReviewService::new(review: Arc<dyn RepositoryReview>)` with `changes`, `stage`, `unstage`, `fingerprint` methods. The service remembers, per repository root, the set of paths in the most recently returned stageable `ChangeSet`; `stage`/`unstage` of a path not in that set → `ReviewError::NotStageable` (safety boundary from the design doc). A non-stageable change set clears the set for that root.
- [ ] `ApplicationError::Review(#[from] ReviewError)`; stable JSON-RPC codes in the domain range: NotARepository `-32020`, GitUnavailable `-32021`, GitFailed `-32022`, NotStageable `-32023`, Io `-32024` (documented as "MUST NOT be renumbered", like `files_rpc_code`).
- [ ] Dispatcher routes: `file-viewer.changes` (params `DiffSpec`), `file-viewer.stage` (`{ repository_root, path, blob: string | null, mode }`), `file-viewer.unstage` (`{ repository_root, path }`), `file-viewer.fingerprint` (`{ repository_root }` → `{ fingerprint }`).
- [ ] Dispatcher test builder gets a `FakeRepositoryReview`; tests: each method routes and parses params; staging a path outside the last change set returns code `-32023`; missing params error names the method.
- [ ] `quantumd` constructs `GitRepositoryReview` and passes `ReviewService` to the dispatcher. `cargo build -p quantumd` succeeds.
- [ ] `cargo test -p quantum-application`, `cargo fmt --all -- --check`, and `just lint` pass.

Steps: service unit tests with a fake port → fail → implement service + error → dispatcher tests → fail → implement routes → wire quantumd → build → commit `feat: expose repository review over IPC`.

---

## Phase 5 — Frontend diff mode

All new pure modules live in `$LIB/diff/` with a sibling `*.test.ts`. Port logic from the playground where named; the playground's behavior was verified headlessly (copy, staging, collapse, rename, large file).

### Task 21: Dependency and IPC types

**Files:**
- Modify: `$VIEW/package.json` (via pnpm), `src/ui/pnpm-lock.yaml`
- Create: `src/ui/packages/client/src/review.ts`; Modify: `src/ui/packages/client/src/index.ts` (re-export)

**Acceptance Criteria:**
- [ ] `./scripts/devsh.sh bash -c "cd src/ui && pnpm --filter file-viewer-panel add diff@^9"` adds `diff` to dependencies; no `@types/diff`.
- [ ] `review.ts` mirrors Task 13 by hand (no codegen exists): `DiffSpec`, `FileSide`, `ChangedFile`, `ChangeSet`, `ReviewErrorKind` (string union of the five kinds), plus `StageParams`, `UnstageParams`, `FingerprintResult`. Field names are `snake_case` exactly as serialized.
- [ ] `index.ts` re-exports them; the client package builds and its tests pass. **Do not touch `shown.ts`/`shown.test.ts` or other uncommitted client changes from the other session; stage only `review.ts` and the `index.ts` hunk you added (`git add -p` on index.ts if it has foreign changes).**

Commit: `feat: add repository review IPC types and diff dependency`.

### Task 22: Line diff and intra-line emphasis

**Files:** Create `$LIB/diff/lineDiff.ts`, `$LIB/diff/intraline.ts` (+ tests)

**Acceptance Criteria:**
- [ ] `lineDiff(oldLines: string[], newLines: string[]): DiffItem[]` where `DiffItem = { type: 'equal'; oldIndex: number; newIndex: number } | { type: 'change'; removed: number[]; added: number[] }`, built from `diffArrays` (package `diff`); consecutive removed/added chunks merge into one `change`; indices are zero-based.
- [ ] `intraLine(oldText: string, newText: string): { removed: [number, number][]; added: [number, number][] } | null` from `diffWordsWithSpace`; returns `null` when `2 × sharedCharacters / (oldText.length + newText.length) < 0.4`; adjacent ranges merge.
- [ ] Tests: identical arrays → only `equal`; pure insertion/deletion; a replaced block yields one change with both lists; the playground's Rust example (`let ext_lower = extension.to_lowercase();` vs `let extension_lower = extension.to_ascii_lowercase();`) marks `ext_lower`/`extension_lower` and `to_lowercase`/`to_ascii_lowercase` only; dissimilar lines → `null`.

Commit: `feat: add line and word diff for the file viewer`.

### Task 23: Whole-file highlighting split into per-line tokens

**Files:** Create `$LIB/diff/highlightLines.ts` (+ test)

**Acceptance Criteria:**
- [ ] `highlightLines(content: string, language: string | undefined): DiffToken[][]` with `DiffToken = { text: string; classes: string }`. Without a language, returns plain tokens (no auto-detection: wrong guesses are worse than none in a diff).
- [ ] Highlights the **whole** content once via `highlightCode` (`$LIB/highlighter.ts`), then walks the HTML with a span-class stack: `<span class="…">` pushes, `</span>` pops, text is entity-decoded (`&amp; &lt; &gt; &quot; &#x27; &#039;`) and split on `\n`; the stack carries across lines so a multi-line comment colors every line.
- [ ] `renderTokens(tokens: DiffToken[], layers: { ranges: [number, number][]; className: string }[]): string` emits escaped HTML; each layer wraps its ranges (emphasis `ix-add`/`ix-del`, search `search-match`/`search-match-current`) by splitting tokens at range boundaries — never by splicing into generated HTML.
- [ ] Tests: a TypeScript `/* … */` comment spanning three lines → all three lines' tokens carry `hljs-comment`; `a < b && c` round-trips through decode and re-escape; emphasis inside a single token splits it with both pieces keeping the token class; two layers nest without breaking markup (parse the output with `DOMParser`, assert text content unchanged).

Commit: `feat: highlight whole files and split into line tokens for diffs`.

### Task 24: Rows with collapsed context

**Files:** Create `$LIB/diff/rows.ts` (+ test)

**Acceptance Criteria:**
- [ ] Port `buildRows` from the playground with only the settled options: 3 context lines; runs hidden only when more than 2 lines would hide; expanded regions emit `recollapse` rows at **top and bottom**; collapsed-region label = count + scope of the first changed line in the following hunk (scope patterns per language: Rust `fn|enum|struct|impl|mod|trait`, TypeScript/JavaScript `function|class|interface|const|type`, TOML `[section]`, Markdown headings; others none).
- [ ] Output row union: `context {oldIndex, newIndex}`, `removed {oldIndex, emphasis}`, `added {newIndex, emphasis}`, `collapsed {key, hiddenCount, firstNewIndex, scopeLine}`, `recollapse {key, count, position}`.
- [ ] Emphasis pairs removed[i] with added[i] in a change block via `intraLine`.
- [ ] `toSplitRows(rows)` pairs removed/added blocks side by side with `null` fillers (port the playground's split loop).
- [ ] Tests: the playground's `file_viewer.rs` fixture (copy both versions into a test fixture) produces collapsed regions of 19 and 26 lines with scopes `pub enum ViewerFileType {` and `pub fn viewer_file_type_for_extension(…)`; expanding a key yields its lines framed by two recollapse rows; a 2-line run never collapses; split pairing with uneven blocks fills with `null`.

Commit: `feat: build diff rows with collapsible context`.

### Task 25: Review model (Unstaged / Staged entries)

**Files:** Create `$LIB/diff/reviewModel.ts` (+ test)

**Acceptance Criteria:**
- [ ] `reviewEntries(changeSet: ChangeSet): ReviewEntry[]` ports the playground's `reviewEntries` (sections mode only). Stageable sets: Unstaged entry (index → target, or base → target when index is absent for tracked files; empty → target for untracked) when index ≠ target or untracked; Staged entry (base → index) when base ≠ index. Equality compares `blob` ids, not content. Non-stageable sets: one entry per file (base → target).
- [ ] `ReviewEntry = { id, fileId, path, oldPath?, language?, status: 'M'|'A'|'D'|'R'|'U', section: 'unstaged'|'staged'|'none', partiallyStaged: boolean, oldSide?: FileSide, newSide?: FileSide }`. Unstaged entries sort before staged ones; within a section, path order.
- [ ] `stagingProgress(entries) → { staged: number; total: number }` (a file counts when it has no unstaged entry).
- [ ] `applyLocalStage(changeSet, path)` / `applyLocalUnstage(changeSet, path)` return a new ChangeSet with `index` set to the displayed target side / to the base side (or removed and `untracked` restored when there is no base). These mirror what git now holds (Task 18 proves it), so the view updates without refetching and without pulling in unseen edits.
- [ ] Tests mirror the playground repository: partial file appears in both sections with `partiallyStaged`; untracked → `U`; rename → `R` with `oldPath`; deletion → `D`; progress counts; local stage/unstage transitions.

Commit: `feat: derive unstaged and staged review entries`.

### Task 26: Clean clipboard text

**Files:** Create `$LIB/diff/copyText.ts` (+ test)

**Acceptance Criteria:**
- [ ] `buildClipboardText(cells: SelectedCell[], options: { layout: 'unified' | 'split'; lockedSide: 'old' | 'new' | null }): string` where `SelectedCell = { kind: 'code'; text: string; side: 'old' | 'new'; rowKind: 'context' | 'removed' | 'added' } | { kind: 'hidden'; lines: string[] }` (cells already trimmed to the selection's start/end offsets by the DOM adapter).
- [ ] Rules (settled): split → only cells on `lockedSide`; unified → drop `removed` cells; `hidden` cells count only when they sit between the first and last kept code cell; join with `\n`; empty code cells keep their blank line.
- [ ] `collectSelectedCells(selection: Selection, root: HTMLElement): SelectedCell[]` (DOM adapter, port of the playground's `selectedCodeText`) reads `.code` cells and `.collapsed` rows in document order using `range.intersectsNode` **only over those elements** (bounded by visible rows, not all text nodes), trimming the first/last cells by text offset.
- [ ] Tests port the three headless cases verified in the playground: contiguous copy across a collapsed region (35 lines, new side only); split view old side vs new side; a removed line inside a unified selection is skipped. Plus: gutter text never appears (cells carry only code text by construction).

Commit: `feat: build clean clipboard text from diff selections`.

### Task 27: DiffRows component

**Files:** Create `$LIB/diff/DiffRows.svelte` (+ test)

**Acceptance Criteria:**
- [ ] Props: `rows`, `layout: 'unified' | 'split'`, `oldTokens`, `newTokens`, `searchRanges` (per side and line), `onExpand(key)`, `onRecollapse(key)`.
- [ ] Markup/CSS per the settled spec and playground: 13px mono, line-height 1.6, old+new number columns (muted 50%, border-right), 3px edge bar on changed rows, 14% tint via `color-mix(in oklab, var(--color-accent|--color-error) 14%, transparent)`, word emphasis 35%, collapsed rows show `↕ N unchanged lines` plus the highlighted scope, recollapse bars `Hide N expanded lines`.
- [ ] `.diff-root { user-select: none }` and only `.code { user-select: text }`. In split view a `pointerdown` on a half adds `pick-old`/`pick-new` to the rows root so the other side's `.code` becomes unselectable; the chosen side is exposed for the copy handler.
- [ ] Split view: no wrapping; each side scrolls horizontally and the two are kept in sync (one `scroll` handler mirroring `scrollLeft`).
- [ ] A `copy` listener on the root uses Task 26 and `event.clipboardData.setData('text/plain', …)` + `preventDefault()`.
- [ ] Tests: unified renders numbers/markers/tint classes; split renders filler halves for uneven blocks; clicking a collapsed row calls `onExpand`; recollapse rows call `onRecollapse`; dispatching a `copy` event over a programmatic selection yields Task 26's text.

Commit: `feat: render unified and split diff rows`.

### Task 28: DiffFile component

**Files:** Create `$LIB/diff/DiffFile.svelte` (+ test)

**Acceptance Criteria:**
- [ ] Sticky 32px header (`--color-bg-alt`): collapse chevron, status letter (M amber `#ffcb6b` as in the playground — or the closest theme token if one exists; U `#89ddff`, R `#82aaff`, A accent, D error), path with dimmed directory (renames: `old → new`), `partially staged` pill, `+N −M`, and — when the set is stageable — a `Stage` / `✓ Staged` button.
- [ ] Staged entries start collapsed; clicking the header toggles.
- [ ] Placeholders: binary (`Binary file changed`), too large (`Too large to diff`), large diff (`additions + deletions > 1000`: dashed `Large diff: N changed lines, not rendered yet.` with a `Load diff` button).
- [ ] Emits `onStage(entry)`, `onUnstage(entry)`; never calls IPC itself.
- [ ] Tests: each placeholder; button label by section; staged entry collapsed by default; header click toggles; rename path text.

Commit: `feat: add diff file section with staging controls`.

### Task 29: ChangesSidebar component

**Files:** Create `$LIB/diff/ChangesSidebar.svelte` (+ test)

**Acceptance Criteria:**
- [ ] 220px; title `Changes` with file count; group headings `Unstaged N` / `Staged N` (stageable sets only); each entry: checkbox (stageable only; checked for staged), status letter, filename, `renamed from …` or directory (ellipsized from the left), partial dot, `+N −M`; active entry in accent.
- [ ] Clicking an entry calls `onSelect(index)`; the checkbox calls `onStage`/`onUnstage` without selecting.
- [ ] Tests: groups and counts; checkbox does not trigger select; active class follows the `activeIndex` prop.

Commit: `feat: add changes sidebar for diff mode`.

### Task 30: DiffView — loading, header, banner, staging, search, keys, ruler

**Files:** Create `$LIB/diff/DiffView.svelte`, `$LIB/diff/pairChangeSet.ts` (+ tests)

**Acceptance Criteria:**
- [ ] Props: `source: { kind: 'git'; spec: DiffSpec } | { kind: 'pair'; left: string; right: string }`.
- [ ] Git: `file-viewer.changes` once on mount (`$effect` with a cancelled flag, like `App.svelte:97-120`). Pair: two `file-viewer.read` calls, combined by `pairChangeSet(left, right): ChangeSet` (`stageable: false`, one file, labels `a ↔ b`).
- [ ] Error states (full window, house style of `App.svelte:195-199`): not a repository, git unavailable (message names quantumd's PATH), git failed (stderr), read failure. Empty set → `No changes`.
- [ ] Header (copy `Header.svelte` styles): `±` badge, `HEAD → working tree` / range / `a ↔ b`, repository path, `n of m staged` + 48px bar (stageable only), total `+N −M`, Unified/Split toggle (default unified), close (`view.hide` with `selfViewName()`).
- [ ] Body: `ChangesSidebar` (git) + one scroll pane with all `DiffFile`s stacked; the sidebar's active entry tracks the file at the top of the pane; clicking an entry scrolls to it. `OverviewRuler` on the right with change marks (one mark per contiguous added/removed/mixed run, measured with Task 9) plus search marks.
- [ ] Staging: `onStage` → `file-viewer.stage { repository_root, path, blob: newSide?.blob ?? null, mode }` then `applyLocalStage`; `onUnstage` → `file-viewer.unstage` then `applyLocalUnstage`; failures show an error banner with the message; no refetch (content never moves).
- [ ] Live changes (git sources whose target is the working tree): record `file-viewer.fingerprint` after load; poll every 2000 ms while mounted; a different value shows the banner `Files changed on disk since this diff loaded.` with `Refresh (R)` (a fingerprint cannot count files, so the text carries no number); refresh refetches changes and re-baselines the fingerprint; the interval is cleared on teardown.
- [ ] Keys (Task 31 resolver): `n`/`p` scroll to the next/previous change block start below the sticky header; `]`/`[` next/previous file; `s` stages/unstages the active file; `r`/`R` refresh; ignored while focus is in an input. A 24px hint bar lists `n p change · ] [ file · s stage / unstage · R refresh · Ctrl+F search · Esc close` (`s`/`R` only for stageable).
- [ ] Search: integrates with App's existing search props (`query`, `currentMatchIndex`, `navigationRevision`, `onMatchCount`, `onMatchPositions`); matches are computed per file over row texts (`findMatchesInLines`), including lines inside collapsed regions and collapsed files; navigating to such a match expands the region/file first, then centers it; marks render through `renderTokens` layers.
- [ ] Tests (mock client like `App.test.ts`): renders entries from a fixture ChangeSet; stage calls IPC with the displayed blob and moves the file to Staged without a refetch; a stage failure shows the banner; fingerprint change shows the refresh banner (fake timers) and R refetches; pair mode renders without sidebar/staging; `n` scrolls to the next change (stub `offsetTop`); search counts include collapsed lines and navigation expands them.

Commit: `feat: add diff view with staging, refresh, search, and keys`.

### Task 31: App routing and keymap

**Files:** Modify `$VIEW/src/App.svelte`, `$LIB/viewerKeymap.ts` (+ tests)

**Acceptance Criteria:**
- [ ] `__quantum_args.diff` → `DiffView` git source (`{ repository, base ?? 'HEAD', target ?? null }`); `__quantum_args.compare` → pair source; `__quantum_args.path` → today's viewer, unchanged.
- [ ] Search bar, Ctrl+F, Enter/Shift+Enter, and Escape work in diff mode exactly as for files (Escape closes search first, then the window).
- [ ] `resolveViewerShortcut` gains (diff-only, reported unconditionally; App decides): `next-change` (`n`), `previous-change` (`p`), `next-file` (`]`), `previous-file` (`[`), `toggle-stage` (`s`), `refresh` (`r`/`R`) — none with Ctrl/Meta/Alt.
- [ ] Existing App tests pass; new tests cover routing for all three arg shapes and that `n` while the search input is focused types into the input instead of navigating.

Commit: `feat: route qv diff and compare arguments to the diff view`.

---

## Phase 6 — Packaging, documentation, real-path verification

### Task 32: qv wrapper forms and git on the daemon PATH (config repo)

**Repository:** `code/infra/config` (separate git repository; follow the `nixos-config-change` skill). **File:** `modules/features/home/quantum.nix` (`wrapProgram` at `:140-142`, `qv` script at `:150-158`).

**Acceptance Criteria:**
- [ ] The `makeWrapper --prefix PATH` list gains `pkgs.git`.
- [ ] `qv` builds its `--args` JSON with `jq` (absolute store path via `${pkgs.jq}/bin/jq`), fixing today's unsafe string interpolation of the path into JSON:
  ```bash
  usage() { echo "Usage: qv <file> | qv <a> <b> | qv --diff [<ref>|<a>..<b>]" >&2; exit 1; }
  [ "$#" -eq 0 ] && usage
  instance="$(date +%s%N)"
  if [ "$1" = "--diff" ]; then
    range="''${2:-HEAD}"
    case "$range" in
      *..*) base="''${range%%..*}"; target="''${range#*..}" ;;
      *)    base="$range"; target="" ;;
    esac
    args="$(jq -n --arg repository "$PWD" --arg base "$base" --arg target "$target" \
      '{diff: {repository: $repository, base: $base, target: (if $target == "" then null else $target end)}}')"
  elif [ "$#" -eq 2 ]; then
    args="$(jq -n --arg left "$(realpath "$1")" --arg right "$(realpath "$2")" '{compare: {left: $left, right: $right}}')"
  elif [ "$#" -eq 1 ]; then
    args="$(jq -n --arg path "$(realpath "$1")" '{path: $path}')"
  else
    usage
  fi
  exec quantumctl show "plugin/file-viewer/file-viewer#$instance" --args "$args"
  ```
  (`''${` is Nix's escape for a literal `${` inside an indented string; keep every command as an absolute store path like the existing script.)
- [ ] The quantum flake input is bumped to the quantum commit containing Phases 1-5; `just build` succeeds.
- [ ] **Hand off to Ross for `just switch`** — never run it.
- [ ] After Ross switches: `systemctl --user show quantum.service -p ExecStart` points at the new wrapper, and `/proc/<quantumd pid>/environ` PATH contains a git store path.

Commit (config repo, path-scoped): `feat: teach qv diff and compare forms; put git on quantumd PATH`.

### Task 33: Documentation

**Files:** quantum `AGENTS.md` (file-viewer entry: diff mode, `qv` forms, staging is the one write action, the four IPC methods; onion table + crate count for `quantum-git`; git joins the service-PATH note), exocortex root `AGENTS.md` "Showing a document to Ross — use qv" (add `qv --diff` for reviewing agent changes before a commit), `brain/projects/` entry for quantum if one tracks qv features.

**Acceptance Criteria:**
- [ ] Every new IPC method, arg shape, and the D1/D2 rules are documented where future agents look first.
- [ ] Router files stay under their word budgets (root `AGENTS.md` router style).

Commit per repository, path-scoped: `docs: document qv diff mode`.

### Task 34: Real-path verification

**Acceptance Criteria:**
- [ ] Full suites green: file-viewer vitest, client package, `cargo test --workspace` (after `cargo build -p quantumd`, per CI), `cargo fmt --all -- --check`, `just lint`, `just frontend-build`.
- [ ] Headless WebKit check of the built view is not required; behavior is covered by tests.
- [ ] Dev daemon run per `AGENTS.md` (`systemctl --user stop quantum.service`, `systemd-run --user --unit=quantum-dev …`, `QUANTUM_PLUGIN_DIR=src/ui/plugins`), with a scratch repository in `/tmp/opencode/qv-diff-scratch` containing every file state from Task 16.
- [ ] Verified through IPC without a window: `quantumctl` (or the IPC socket) calls `file-viewer.changes`/`stage`/`unstage`/`fingerprint` and the results match `git status` after each step, including the D1 proof (edit after load, stage, `git show :path` equals the original).
- [ ] **Ask Ross before opening any window**, then open `qv --diff` in the scratch repository once and walk him through it one case at a time (his acceptance-testing preference); also open the murmur8 example Markdown file and confirm Ctrl+F is responsive with markers on the ruler.
- [ ] Restore the installed daemon: `systemctl --user stop quantum-dev && systemctl --user start quantum.service`.

---

## Parallelization map (for subagent-driven execution)

| Track | Tasks | Depends on |
|---|---|---|
| A — search performance | 1 → 2, 3 | none |
| B — side fixes | 4, 5, 6 | none (5/6 touch the same renderers as 3 and 11: run after 3, before 11) |
| C — ruler | 7, 8, 9 → 10 → 11 → 12 | 2, 3, 5, 6 |
| D — Rust backend | 13 → 14 → 15 → 16 → 17, 18, 19 → 20 | none |
| E — diff frontend | 21 → 22, 23, 24, 25, 26 → 27, 28, 29 → 30 → 31 | 7-10 (ruler), 20 (IPC shapes; mockable earlier) |
| F — ship | 32, 33, 34 | all |

Tracks A, B and D are independent and can run concurrently with separate owners; give each worker this plan, the design doc, the playground path, its exact file list, and the rule to stage only its own paths.

