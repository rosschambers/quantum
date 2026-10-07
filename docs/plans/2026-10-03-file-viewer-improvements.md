# File Viewer and Explorer Improvements — Implementation Plan

> **For OpenCode:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

Design: `docs/plans/2026-10-03-file-viewer-improvements-design.md` records the original
evidence and non-goals. The image-route trust boundary is now settled by the isolated
native reproduction and WebView-identity checks; incremental directory loading remains
out of scope, not a pending implementation choice. See execution status below for the
current outcome; the task bodies retain the original implementation sequence.

**Goal:** Make the explorer responsive on large directories by removing duplicate listing
work; give `qv` (the file viewer) literal in-file search across text, code, JSON, and
rendered Markdown; fix `qv`'s image view (zoom/fit/actual-size/pan) after reproducing its
current load behavior — without redesigning virtualization, without regex search, and
without any arbitrary file-read endpoint.

**Original architecture:** Phase A is backend-only (`quantum-files` infrastructure crate +
`quantum-application`'s `FilesService`), reusing the reference-counted `Subscription`
pattern already proven for `watch_handles`/`size_handles`. Phase B is frontend-only in the
file-viewer plugin (`src/ui/plugins/file-viewer/views/file-viewer`), adding a pure keymap
resolver mirroring the explorer's `keymap.ts`, plus pure match-finding functions with a
DOM-walk exception for Markdown (the only content type whose "visible text" exists only as
rendered HTML). Phase C touches `ImageRenderer.svelte` and, only if Task 11's reproduction
confirms a real failure, runs an architecture spike (Task 12) before building anything: the
preferred fix is an exact-resource, WebView-identity-bound capability route entirely inside
`quantum-ui` (`bridge.rs` issues a grant bound to the requesting `WebView`; `scheme.rs`
resolves it only for a matching `WebView` via `URISchemeRequest::web_view()`; `registry.rs`
revokes it on window close) — no path-based or extension-only route, no new size cap, under
any framing. A `data:` URI is a valid simpler alternative only if measured to have no
material cost and documented with that measurement — never a default, and never paired
with a new cap.

**Tech Stack:** Rust (Tokio, `async-trait`) for Phase A and, in Phase C, `quantum-ui`
(`bridge.rs`, `scheme.rs`, `registry.rs`) for the capability route or `quantum-files` if the
data: URI alternative is chosen instead. Svelte 5 runes + TypeScript + vitest
(`@testing-library/svelte/svelte5` adapter, `jsdom` environment — both already configured
in this plugin's `vite.config.ts` and listed in `package.json`, no new dependency) for
Phases B and C's frontend work. Rust commands run through `./scripts/devsh.sh` per this
repo's `AGENTS.md`; frontend commands run from each view's own directory (the pnpm
workspace root is `src/ui`, not the repo root).

**Original assumptions (outcomes recorded in execution status):**
- The explorer's triplicate-listing bug and the `/etc/passwd`-per-entry cost are confirmed
  by direct source reading (design doc); whether they are the dominant cause of the
  user-perceived freeze on the real installed binary is NOT assumed — Task 3 measures
  live, it does not declare victory after Tasks 1-2.
- The standalone image view's `file://` URI (`read_for_viewer_blocking`'s
  `ViewerFileType::Image` arm) may or may not actually fail to load under WebKit — it is
  suspected, not proven, for this specific call path (markdown's own image handling in this
  same backend function already avoids `file://` for the common case via
  `embed_markdown_images`, but the standalone-image arm has no equivalent rewrite) — Task 11
  reproduces it before Phase C builds anything.
- `webkit6::URISchemeRequest::web_view()` is confirmed to exist in this repo's exact pinned
  version (`webkit6 0.4.0`, `Cargo.lock`) — but whether it reliably identifies the correct
  originating `WebView` for a real sub-resource load in this daemon is NOT yet verified;
  Task 12's architecture spike checks this live before Task 13 builds on it. If the spike
  finds it does not hold, Task 13 does not proceed with a weaker substitute — it stops for
  a revised architecture decision from Ross.
- `@testing-library/svelte/svelte5` plus a top-level `$effect` (not `onMount`) is the only
  combination confirmed working for Svelte 5 keyboard-integration tests in this repo
  (explorer's `Toolbar.test.ts`); Task 4 confirms this holds for this plugin's `App.svelte`
  before later tasks rely on it.
- No task commits, pushes, merges, or touches Nix packaging. Every task ends with tests
  green locally; committing is deferred to the final section.

---

## Execution status (2026-10-07)

Implementation and local verification were completed in `.worktrees/qv-improvements`,
branch `feat/qv-improvements`. Ross subsequently authorized committing, merging to
`main`, and pushing. The implementation and documentation commits are integrated and
pushed: `69463c3b`, `0661131f`, `8c3973b7`, `4be58c1c`, `c4033684`, and `8b1fcd34`.
Canonical `main` and remote `main` were verified at `8b1fcd34` during this checkpoint.
Ross reported completing `just switch` and accepted the live walkthrough below.
This supersedes the earlier deployment-pending and interaction-open status. A separate
running-process derivation check matched its source hash to the configuration pin,
as recorded below.
Durable usage/release notes and reproducible checks are in the repository's existing
`README.md` and `docs/development.md`.

- **Explorer:** owner lookup now reads `/etc/passwd` once per listing, and concurrent
  same-path calls share one filesystem walk with settled entries removed. Sequential
  validation calls still perform fresh walks; this is not a persistent listing cache.
  The directory tree now virtualizes rows, preserving the existing file-list
  virtualization. Shared live updates and batched sizes remain; pane generations guard
  stale reload/navigation/teardown results. The original backend-only scope expanded
  to address measured tree rendering cost, not to add pagination.
- **Search:** literal, case-insensitive Ctrl+F search covers text, code, JSON, and
  rendered Markdown, with match counts, wrapping navigation, Escape-before-close,
  virtualized-line reveal, and containing-fold expansion that preserves unrelated folds.
  Diagram source/SVG is excluded. Component and native built-bundle checks cover the
  implemented paths; Ross also accepted the live search walkthrough below.
- **Images:** native reproduction confirmed the `file://` load failure from a
  `quantum://` page and distinct originating WebView identities. The chosen transport
  uses exact open-file grants owned by the requesting WebView, revoked on new reads,
  navigation, hide, and destruction; no arbitrary file endpoint or new size cap.
  Fit, Actual size in CSS pixels, zoom, pan, and visible load errors are implemented.
  Native built-bundle observations confirmed PNG, JPEG, GIF, WebP, SVG, BMP, ICO, and
  AVIF loaded, including special-character paths. Pan has component coverage and Ross
  accepted its desktop behavior on large PNG and SVG fixtures; that owner acceptance
  is separate from the native test.

### Historical measurements and coverage limits

The 10,000-entry Chromium experiment used mocked IPC, not the installed explorer or
an integrated backend. These are sample timings, not service performance guarantees:

| Measurement | Before | After |
| --- | ---: | ---: |
| Rendered tree rows | 9,463 | 52 |
| DOM elements | 76,832 | 1,545 |
| Startup | 1,450 milliseconds | 350 milliseconds |
| Size-update processing | 600 milliseconds | 260 milliseconds |
| Scroll | 67 milliseconds | 17 milliseconds |
| Selection | 98 milliseconds | 123 milliseconds |

Selection did not improve in this mocked experiment. These numbers do not supply
Task 3's separated real listing/IPC/render measurements or prove installed performance.
Ross subsequently accepted the live explorer experience; the historical timings are
not an open blocker or a new follow-up task. The native host test's 9.05-second suite
duration is separate evidence, not a comparable explorer startup measurement.

### Automated verification and integration

The parent execution's fresh checks passed: recursive frontend build, `quantumd` build,
full Rust workspace tests, Rust formatting, all 1,025 frontend tests (viewer 157;
explorer 304), workspace/all-target Clippy with warnings denied, and `git diff --check`.
Exact commands are recorded in the canonical repository's `docs/development.md`.
Existing modal accessibility warnings outside these features and the Vite chunk-size
advisory remain non-failing; no Rust full-suite count is inferred.

The ignored `quantum-ui` native test (`gtk-test`, `native_file_image`) passed once against
the actual built viewer bundle in private Xvfb/DBus sessions. Evidence:
`/tmp/opencode/quantum-warm-hide-bundle-final.log`; main JavaScript SHA-256:
`1db00c67c6116179898c821347a99df2827b354967d2a43709837eb139227a79`.
The parent repeated the integrated native check after the recursive frontend rebuild:
one test passed in 11.95 seconds, including all eight image formats and the unchanged
full-visibility assertion. That rebuilt entry was `assets/index-B-3wlxaj.js`, SHA-256
`7fe4085e9bf88260a26f8030d7495497d01ea410cbf274b67de669873d08190d`.
It covers real host bridge/grants and built frontend with a fixture dispatcher, not
the installed daemon. Eight-format coverage required external fixtures and inspection
of successful decode observations; the default test alone does not certify all eight.
The development guide gives commands independent of this temporary log or runner.

Canonical `main` retains unrelated dirty host/menu/client work. During integration,
the overlapping `src/ui/host/src/windows/mod.rs` and
`src/ui/host/src/windows/panel.rs` changes were temporarily preserved, restored cleanly,
and checked against their original diffs. They are not included in these commits.
The commit prohibition in the original task instructions below is superseded by Ross's
explicit commit/merge/push authorization. Ross separately authorized committing this
checkpoint and the configuration pin; no additional activation was performed.

### Live owner acceptance (2026-10-07)

Ross reported that he had already run `just switch`. The parent session opened each
viewer fixture separately with `qv`, waiting for approval before the next. All passed:

| Fixture or view | Owner-approved behavior |
| --- | --- |
| `01-search.md` | Ctrl+F: three `needle` matches; one `hello world` match across inline formatting; zero diagram-only matches; Escape closes search before the viewer. |
| `02-long-text.txt` | Four matches at lines 20, 550, and twice on 950; scrolling keeps matches fully visible and search reveals them again after manual scrolling. |
| `03-folds.json` | Enclosing ancestors expand for a match while unrelated folds stay collapsed. |
| Large PNG, 2400 by 1600, with spaces, `#`, `%`, and an accented character in its name | Fit, Actual size at 100%, zoom, and panning to every corner. |
| `04-large-image.svg` | The same Fit, Actual size, zoom, and all-corner pan checks. |
| `05-broken.png` | Visible load error and Escape dismissal. |
| Explorer, opened at `/tmp` as `plugin/files/files#<timestamp>` | Both lists scroll; fixture-folder and child navigation, Back, Forward, and Alt+Up work. |

Ross's final "cool it all works checkpoint the session" accepts the directory test
as well as the preceding viewer checks. The manual pack lived under
`/tmp/opencode/qv-manual-tests/`; it is temporary and may need regeneration, not a
durable repository artifact. No additional feature work was identified.

Separately, the checkpoint observed `quantum.service` active/running on x1 and resolved
its executable to `/nix/store/jalif7q6qzzc84msd6fpbqbpkpsdw9mq-quantum-0.1.0/bin/.quantumd-wrapped`.
Following its Nix derivation through the frontend derivation identified
`/nix/store/cckgdv2fyihq5awy7h7fcgsv6jsjf8gr-source`. Its `nix hash path` result was
`sha256-09qkWd1Iw9QNgUKUwtptv1Xj/DEKR6QGkWavt1qBSdw=`, matching the working configuration
lock's Quantum input for `8b1fcd34df44a8e757ddc6fa3b11ca1b7ea03003`. This establishes the
running package's source independently of the acceptance report. Configuration commit
`0c21d38` records that already-activated pin; unrelated staged configuration work was
excluded, and `just check` passed before the path-scoped commit.

---

## Phase A — Explorer directory-listing responsiveness

### Task 1: Pure `/etc/passwd` parser, read once per listing

**Files:** Modify `src/infrastructure/files/src/filesystem.rs`.

**Acceptance criteria:**
1. New pure function `parse_passwd(contents: &str) -> HashMap<u32, String>`, same
   `name:password:uid:...` line format `owner_from_passwd` already parses.
2. `owner_from_passwd` becomes one `read_to_string` + `parse_passwd`; behavior unchanged.
3. `entry_from_path` accepts `owner_map: Option<&HashMap<u32, String>>`: `Some` looks up
   directly (falling back to the numeric uid on a miss, as today); `None` reads the file
   itself (preserves `stat`'s existing single-entry behavior unchanged).
4. `list_directory_blocking` loads the map once, before its entry loop, and passes it to
   every `entry_from_path` call. `FileSystemPort::stat` is unaffected.

**Step 1 (red)** — add to `mod tests` (`filesystem.rs:766`):
```rust
#[test]
fn parse_passwd_extracts_name_by_uid() {
    let contents = "root:x:0:0:root:/root:/bin/bash\nalice:x:1000:1000:Alice:/home/alice:/bin/bash\n";
    let map = parse_passwd(contents);
    assert_eq!(map.get(&0), Some(&"root".to_string()));
    assert_eq!(map.get(&9999), None);
}

#[tokio::test]
async fn list_directory_resolves_owner_consistently_across_entries() {
    let dir = tempfile::tempdir().expect("create tempdir");
    fs::write(dir.path().join("a.txt"), b"a").expect("write a");
    fs::write(dir.path().join("b.txt"), b"b").expect("write b");
    let entries = LocalFileSystem::new()
        .list_directory(&dir.path().to_string_lossy())
        .await
        .expect("list directory");
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].owner, entries[1].owner);
}
```
Run `./scripts/devsh.sh cargo test -p quantum-files parse_passwd` and
`... list_directory_resolves_owner` — both FAIL (function missing / entries not yet
guaranteed to agree once the map path and the single-entry path can diverge).

**Step 2 (green):** extract `parse_passwd`; thread `owner_map` through as described. Run
`./scripts/devsh.sh cargo test -p quantum-files` — all green, including the five
pre-existing owner-adjacent tests.

---

### Task 2: Coalesce concurrent same-path `files.list` requests, no persistent cache

**Files:** Modify `src/application/src/use_cases/files_service.rs`.

**Acceptance criteria:**
1. `FilesService::list` shares one in-flight filesystem call across concurrent callers for
   the same path (an in-flight map keyed by path; any concurrent primitive that satisfies
   the tests below is acceptable — for example `Mutex<HashMap<String,
   Arc<tokio::sync::OnceCell<Result<Vec<FileEntry>, FilesError>>>>>`).
2. Once a listing settles (success or error), its in-flight entry is removed immediately —
   no TTL, no persistent cache; a later call for the same path always re-walks the disk.

**Step 0 — extend the real test fixtures first (no assertions yet).** The real test
constructor in this module is `build_service(filesystem: FakeFileSystem, watcher:
FakeWatcher, sizer: FakeSizer, pins: FakePins, applications: FakeApplications) ->
(FilesService, Fakes)` (`files_service.rs:680-699`) — there is no `make_service`. Add to
`FakeFileSystem` (currently `entries, drives, stat_entry, text_preview, image_preview,
perform_error`, `files_service.rs:445-465`) four new fields defaulted to inert values:
`list_calls: Arc<AtomicUsize>`, `entered: Arc<tokio::sync::Notify>`, `proceed:
Arc<tokio::sync::Notify>`, and `gate_listing: bool` defaulting to `false`. The test module imports `Ordering` already (via `use super::*`,
which brings in the parent module's `std::sync::atomic::{AtomicU64, Ordering}`) but not
`AtomicUsize` — add `use std::sync::atomic::AtomicUsize;` to `mod tests`'s imports. In
`list_directory`, increment `list_calls`, call
`self.entered.notify_one()`, then await `self.proceed.notified()` only when
`gate_listing` is `true`, before returning
`Ok(self.entries.clone())`. This handshake is required because the existing fake resolves
instantly with no `.await` suspension point — two instantly-resolving futures run to
completion sequentially even under `tokio::join!`, so that setup cannot test sharing
of genuinely overlapping requests. The handshake
forces genuine overlap: the test knows the first call has truly entered the fake (via
`entered`) before it lets the second call proceed, and nothing resolves until the test
explicitly releases `proceed`.

**Step 1 (red):**
```rust
#[tokio::test]
async fn concurrent_list_requests_for_same_path_share_one_filesystem_call() {
    let calls = Arc::new(AtomicUsize::new(0));
    let entered = Arc::new(tokio::sync::Notify::new());
    let proceed = Arc::new(tokio::sync::Notify::new());
    let filesystem = FakeFileSystem {
        entries: vec![sample_entry("a.txt", ContentKind::Other)],
        list_calls: calls.clone(),
        entered: entered.clone(),
        proceed: proceed.clone(),
        gate_listing: true,
        ..Default::default()
    };
    let (service, _fakes) = build_service(
        filesystem, FakeWatcher { changes: vec![] }, FakeSizer { updates: vec![] },
        FakePins { pins: vec![] }, FakeApplications { applications: vec![] },
    );
    let service = Arc::new(service);

    let first = tokio::spawn({ let s = service.clone(); async move { s.list("/home/user").await } });
    entered.notified().await; // the first call has genuinely entered the fake
    let second = tokio::spawn({ let s = service.clone(); async move { s.list("/home/user").await } });
    tokio::task::yield_now().await;
    tokio::task::yield_now().await; // give `second` a chance to either coalesce or re-enter the fake

    assert_eq!(calls.load(Ordering::SeqCst), 1, "a second concurrent call must not re-enter the filesystem");

    proceed.notify_waiters();
    let (first, second) = tokio::join!(first, second);
    assert!(first.expect("first task").is_ok());
    assert!(second.expect("second task").is_ok());
}

#[tokio::test]
async fn sequential_list_requests_after_settling_each_hit_the_filesystem() {
    let calls = Arc::new(AtomicUsize::new(0));
    let filesystem = FakeFileSystem { list_calls: calls.clone(), gate_listing: false, ..Default::default() };
    let (service, _fakes) = build_service(
        filesystem, FakeWatcher { changes: vec![] }, FakeSizer { updates: vec![] },
        FakePins { pins: vec![] }, FakeApplications { applications: vec![] },
    );
    service.list("/home/user").await.expect("first");
    service.list("/home/user").await.expect("second");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}
```
Run `./scripts/devsh.sh cargo test -p quantum-application concurrent_list_requests` — FAILS
(`calls == 2` at the assertion point, no sharing yet). The second test already passes
today and pins the no-persistent-cache requirement so Step 2 cannot regress it.

**Step 2 (green):** implement the in-flight map; remove each entry once settled. Run
`./scripts/devsh.sh cargo test -p quantum-application` — all green, including every
pre-existing test in this file (`list_delegates_to_filesystem` and the rest).

---

### Task 3: Live, separated measurement (listing vs IPC transfer vs sort/render)

Evidence-gathering, not a new feature. Capture the baseline using this same measurement
procedure **before Tasks 1-2**, then repeat with Tasks 1-2 applied. Run the dev daemon
(`QUANTUM_PLUGIN_DIR=src/ui/plugins`, coordinate with Ross before opening any window).
Against a real multi-thousand-entry directory, measure separately: (a) backend listing
wall time, (b) `files.list` IPC payload size/transfer time, (c) `FileList.svelte` sort +
initial render time (temporary instrumentation only, removed after). Record all three plus
whether the explorer visibly stalls, opening the same path dual-pane (now one real listing,
not three). **Decision gate:** if (b) or (c) dominates, incremental/paginated loading is a
real candidate — take it to Ross as its own design, per the design doc's non-goal; do not
design it here. If (a) dominates and the stall is gone or much reduced, Phase A is done.

---

## Phase B — `qv` in-file search (Ctrl+F)

### Task 4: Testability spike — does the `onMount` `keydown` listener fire under test?

**Files:** Test: `src/ui/plugins/file-viewer/views/file-viewer/src/App.test.ts` (new).
Modify `App.svelte` only if the spike shows a problem.

**Why this is not a trivial render-and-dispatch test:** `App.svelte`'s `onMount`
(`App.svelte:40-88`) returns EARLY at line 46 when `window.__quantum_args?.path` is
missing — before the `keydown` listener is ever registered — and even on the success path
the listener is only registered AFTER `await client.call('file-viewer.read', { path })`
resolves (lines 51-58, listener added at line 84). A test that renders `App` without first
setting `window.__quantum_args` and mocking that call, or that dispatches `Escape`
synchronously right after `render()`, would get a false negative indistinguishable from
"the harness never fires `onMount`" — exactly the ambiguity this task exists to resolve.
`App.svelte` has no injectable `ipc`/client prop (unlike the explorer's `App.svelte`), so
`createClient` must be mocked via `vi.mock('@quantum/client', ...)`, mirroring the
explorer's `importOriginal`-based pattern (`files/.../App.test.ts:12-15`) but overriding
`createClient` itself rather than `openContextMenu`.

**Acceptance criteria:**
1. The test sets `window.__quantum_args = { path: '/tmp/fixture.txt' }` BEFORE rendering,
   mocks `createClient` to return a `call` stub resolving `'file-viewer.read'` with a
   minimal fixture `ViewerFileInfo` (`file_type: 'text'`) and `'view.hide'` with `undefined`,
   renders `App`, and awaits the initial load actually completing (for example `vi.waitFor`
   polling until the mocked `call` was invoked with `'file-viewer.read'`) before dispatching
   any keyboard event.
2. Only then does it dispatch `keydown` (`key: 'Escape'`) on `window` and assert the mocked
   `call` was invoked with `'view.hide'`.
3. `afterEach` deletes `window.__quantum_args` and clears/restores the `@quantum/client`
   mock so later tests in this file (and Task 8's) start clean.
4. The result (pass/fail) decides whether moving the listener out of `onMount` into a
   top-level `$effect` is a prerequisite before Task 8.

**Step 1 (diagnostic):**
```ts
import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte/svelte5';

const { callMock } = vi.hoisted(() => ({ callMock: vi.fn() }));
vi.mock('@quantum/client', async (importOriginal) => {
    const actual = await importOriginal<Record<string, unknown>>();
    return { ...actual, createClient: () => ({ call: callMock }) };
});

import App from './App.svelte';

afterEach(() => {
    delete (window as any).__quantum_args;
    callMock.mockReset();
});

describe('App keyboard handling', () => {
    it('registers the keydown listener and Escape calls view.hide', async () => {
        (window as any).__quantum_args = { path: '/tmp/fixture.txt' };
        callMock.mockImplementation((method: string) => {
            if (method === 'file-viewer.read') {
                return Promise.resolve({
                    content: 'hello', file_type: 'text', filename: 'fixture.txt',
                    directory: '/tmp', size: 5, language: null, mime_type: null, uri: null,
                });
            }
            return Promise.resolve(undefined);
        });

        render(App);
        await vi.waitFor(() => {
            expect(callMock).toHaveBeenCalledWith('file-viewer.read', { path: '/tmp/fixture.txt' });
        });

        await fireEvent.keyDown(window, { key: 'Escape' });
        expect(callMock).toHaveBeenCalledWith('view.hide', expect.anything());
    });
});
```
Run `cd src/ui/plugins/file-viewer/views/file-viewer && pnpm exec vitest run src/App.test.ts`.

**Step 2:** if it passes, keep it as a permanent regression guard and proceed to Task 5
unchanged. If it fails, move only the `keydown` registration into a top-level `$effect`
(handler bodies unchanged), re-run until green. Do not start Task 8 until this is green for
the right reason.

---

### Task 5: `viewerKeymap.ts` — pure keyboard resolver

**Files:** Create `.../src/lib/viewerKeymap.ts` and `viewerKeymap.test.ts`.

**Acceptance criteria:**
1. `ViewerShortcutAction = { kind: 'open-search' } | { kind: 'next-match' } |
   { kind: 'previous-match' } | { kind: 'close-search' }`; `resolveViewerShortcut(event):
   ViewerShortcutAction | null`.
2. Ctrl/Cmd+F (no shift) → `open-search`; Enter → `next-match`; Shift+Enter →
   `previous-match`; Escape → `close-search` (caller decides what "close" means — see
   design doc's Escape-ordering note). Alt held → `null` (Alt reserved for navigation,
   mirroring the explorer's convention).

**Step 1 (red)**, mirroring `files/.../keymap.test.ts`'s shape exactly:
```ts
test('Ctrl+F and Cmd+F open search; Enter/Shift+Enter navigate; Escape resolves', () => {
    expect(resolveViewerShortcut(key({ key: 'f', ctrlKey: true }))).toEqual({ kind: 'open-search' });
    expect(resolveViewerShortcut(key({ key: 'Enter' }))).toEqual({ kind: 'next-match' });
    expect(resolveViewerShortcut(key({ key: 'Enter', shiftKey: true }))).toEqual({ kind: 'previous-match' });
    expect(resolveViewerShortcut(key({ key: 'Escape' }))).toEqual({ kind: 'close-search' });
    expect(resolveViewerShortcut(key({ key: 'f', ctrlKey: true, altKey: true }))).toBeNull();
});
```
Run `pnpm exec vitest run src/lib/viewerKeymap.test.ts` (from the view directory) — FAIL
(module missing). **Step 2:** implement; re-run — PASS.

---

### Task 6: Pure match-finding — `findMatches` (line-based) and the Markdown DOM matcher

**Files:** Create `.../src/lib/search.ts` and `search.test.ts`.

**Acceptance criteria:**
1. `findMatches(text, query): { start, end }[]` — literal, case-insensitive,
   non-overlapping; empty query → `[]`.
2. `findMatchesInLines(lines, query): { lineIndex, range }[]` — per-line, in order (basis
   for Text/Code/JSON search).
3. `findMatchesInDom(root: Element, query): Range[]` — `TreeWalker` over text nodes,
   skipping any subtree whose nearest element ancestor has class `diagram-block` or
   `diagram-error` (basis for Markdown search — jsdom's `createTreeWalker`, already
   available under this plugin's `jsdom` vitest environment).

**Step 1 (red)** — one illustrative case per function:
```ts
test('findMatches is case-insensitive and non-overlapping', () => {
    expect(findMatches('Foo foo', 'foo')).toEqual([{ start: 0, end: 3 }, { start: 4, end: 7 }]);
});
test('findMatchesInLines reports line index', () => {
    expect(findMatchesInLines(['alpha', 'beta alpha'], 'alpha')).toEqual([
        { lineIndex: 0, range: { start: 0, end: 5 } }, { lineIndex: 1, range: { start: 5, end: 10 } },
    ]);
});
test('findMatchesInDom skips diagram-block subtrees', () => {
    document.body.innerHTML = '<div id="r"><p>needle</p><div class="diagram-block">needle</div></div>';
    expect(findMatchesInDom(document.getElementById('r')!, 'needle')).toHaveLength(1);
});
```
Run `pnpm exec vitest run src/lib/search.test.ts` — FAIL. **Step 2:** implement `search.ts`
— PASS.

---

### Task 7: `foldContaining` lookup and `VirtualScroller` external scroll-to-index

**Files:** Modify `fold-model.ts`, `VirtualScroller.svelte`. Create `fold-model.test.ts`.

**Acceptance criteria:**
1. `foldContaining(model, line): CodeFoldRange | null` — the fold whose
   `[startLine, endLine]` contains `line`; `null` if none, or if `line` is itself a fold's
   `startLine` (a fold's own header line is always visible).
2. `VirtualScroller` gains an optional prop `scrollToIndex?: number`; changing it sets the
   container's `scrollTop` to `index * lineHeight` (clamped), via an internal `$effect`.
   Existing user-scroll behavior unchanged.

**Step 1 (red):**
```ts
test('foldContaining finds the fold containing a line, not its header', () => {
    const model = buildCodeFoldModel(['function f() {', '  return 1;', '}', 'g();']);
    expect(foldContaining(model, 1)).toEqual({ startLine: 0, endLine: 1 });
    expect(foldContaining(model, 0)).toBeNull();
});
```
Plus a `VirtualScroller` component test asserting `scrollToIndex` moves the rendered
container's `scrollTop` to the expected pixel offset. Run — FAIL. **Step 2:** implement
both — PASS.

---

### Task 8: Search bar component + wiring in `App.svelte`

**Files:** Create `SearchBar.svelte`, `SearchBar.test.ts`. Modify `App.svelte`.

**Acceptance criteria:**
1. `SearchBar` renders a text input and a "match X of Y" / "No matches" indicator.
2. `App.svelte`: Ctrl/Cmd+F opens (or refocuses) the bar; Enter/Shift+Enter move to the
   next/previous match, wrapping past the last/first; Escape closes the bar first if open,
   and only hides the window (today's behavior) when the bar is already closed.
3. Matches are computed per active renderer (`findMatchesInLines` for Text/Code/JSON,
   `findMatchesInDom` for Markdown); navigating scrolls the match into view (via
   `VirtualScroller.scrollToIndex` when virtualized, else `scrollIntoView`), and when the
   match's line is inside a currently-collapsed fold, expands exactly that fold via
   `foldContaining` — every other fold untouched, expansion stays after the bar closes.

**Step 1 (red)** — illustrative subset; full coverage follows this shape for each criterion
above:
```ts
test('Ctrl+F opens the search bar and focuses its input', async () => {
    const { container } = render(App /* props/mocks: a loaded text file */);
    await fireEvent.keyDown(window, { key: 'f', ctrlKey: true });
    expect(document.activeElement).toBe(container.querySelector('.search-bar input'));
});
test('Escape closes the bar before closing the viewer', async () => {
    const { container } = render(App /* mocks: capture view.hide calls */);
    await fireEvent.keyDown(window, { key: 'f', ctrlKey: true });
    await fireEvent.keyDown(window, { key: 'Escape' });
    expect(container.querySelector('.search-bar')).toBeNull();
    // second Escape now hides the window — assert against the mocked client.call
});
```
Run `pnpm exec vitest run src/App.test.ts src/lib/SearchBar.test.ts` — FAIL. **Step 2:**
implement `SearchBar.svelte` and the `App.svelte` wiring — PASS, including Task 4's
pre-existing Escape test.

---

### Task 9: Render integration — segment highlighting (Code/JSON) and DOM `<mark>` (Markdown)

**Files:** Modify `CodeRenderer.svelte`, `JsonFoldRenderer.svelte`, `TextRenderer.svelte`,
`MarkdownRenderer.svelte`.

**Acceptance criteria:**
1. Each accepts an `activeMatches` input; a matched line/text-node renders wrapped in
   `<mark class="search-match">` (current match also gets `search-match-current`, theme
   tokens only for color).
2. Code/JSON: per-segment `highlightCode` (split plain text at match boundaries, highlight
   each segment independently) — never string-splicing `<mark>` into already-generated
   HTML. The documented cosmetic trade-off (a match mid-token may highlight slightly
   differently at the boundary) is acceptable; match text itself must remain exact.
3. Markdown: DOM `Range`/`<mark>` insertion from `findMatchesInDom`'s output, applied after
   `marked` renders — never by altering `parsedHtml` as a string.

**Steps:** per renderer, one failing test asserting a known query highlights the expected
substring (for example CodeRenderer with `content: 'const needle = 1;'`, query `'needle'`,
asserting the output contains `<mark class="search-match">needle</mark>`), then implement
the minimal split/DOM-mark logic. Run each component's test file from the view directory;
all green before moving to Task 10.

---

### Task 10: Live verification across content types

Manual, live check (coordinate with Ross). Via `qv`, open a Markdown file with a mermaid
fence, a JSON file with nested folds, a code file over 500 lines, a text file over 500
lines. For each: open search, confirm match count/current indicator, confirm wrap at both
ends, confirm a match inside a collapsed JSON fold (and, for ≤500-line code, a collapsed
code fold) reveals and stays expanded after closing search, confirm a match beyond the
500-line virtualization window scrolls into view, confirm Markdown search finds visible
rendered text only — never the mermaid diagram's own source/SVG.

---

## Phase C — `qv` image viewer

### Task 11: Reproduce the current image load behavior (before building anything)

Live check (coordinate with Ross; no unattended window). Open a known-good small PNG via
`qv <path>`. Record exactly what happens: renders / blank / broken-image icon / WebKit
console error (`QUANTUM_INSPECTOR=1` to check the console). This decides whether Task 12 is
needed — if the image renders correctly today, skip Tasks 12-13 (record that outcome
rather than silently dropping it) and narrow Phase C to zoom/pan/fit (Task 14) only.

---

### Task 12: Architecture spike — does `URISchemeRequest::web_view()` identify the right viewer?

**This is evidence-gathering before any production code is written.** Everything below is
already verified by reading source and the pinned crate's published API; what is NOT yet
verified is runtime behavior in this daemon, which is this task's job.

**Already confirmed (cite, do not re-derive):**
- `bridge.rs`'s `connect_script_message_received` closure (`bridge.rs:56-58,97`) already
  holds `webview_clone: WebView` — the exact `WebView` that sent a given IPC call — in
  scope at the point it dispatches. `PanelWindow::new` (`panel.rs:105,271`) also has the
  instance-qualified `view_name` in scope at the point it calls `register_bridge`, if a
  name-based (rather than GObject-identity-based) association is ever preferred.
- `webkit6::URISchemeRequest::web_view(&self) -> Option<WebView>` exists in this repo's
  exact pinned version (`webkit6 0.4.0` per `Cargo.lock`, confirmed against that version's
  own published docs, not a later release).
- `WindowRegistry::destroy_window` / `hide_window` (`registry.rs:644,666`) are in the same
  crate (`quantum-ui`) as `bridge.rs` and `scheme.rs` — a grant store living in `quantum-ui`
  needs no new crate dependency and no domain port to be reachable from all three.

**What this task verifies live, against the real dev daemon (coordinate with Ross):**
1. Instrument (temporarily) `scheme.rs`'s quantum:// handler to log the `web_view()` of an
   incoming request alongside the requesting page's URL, for a handful of real resource
   loads from a `qv` window (for example its own `quantum://plugin/file-viewer/...`
   `index.html`/asset loads, which already flow through this same handler). Confirm it
   returns `Some` and matches the webview that actually owns that page — not `None`, and
   not a different window's webview.
2. Confirm GObject pointer-identity comparison (`PartialEq`/`ObjectExt`, the same category
   already used for `MonitorId` in `registry.rs`) correctly distinguishes two different
   `qv` windows' webviews from each other (open two `qv` instances via the `#<id>` suffix
   per this repo's multi-instance pattern; confirm their `WebView`s compare unequal).

**Decision gate:** if both checks hold, Task 13 builds the capability route using this
mechanism. **If either does not hold** (`web_view()` returns `None` or an incorrect webview
for a real sub-resource load), **stop — do not proceed with Task 13's route, and do not
substitute a weaker token-only or TTL-only scheme.** Write up the finding and take it back
to Ross for a revised architecture decision; only the measured `data:` URI alternative
(see Task 13) remains available until that decision is made.

---

### Task 13: Build the chosen transport — capability route (preferred) or measured `data:` URI

**Files (route path):** Modify `src/ui/host/src/bridge.rs`, `src/ui/host/src/scheme.rs`,
`src/ui/host/src/registry.rs`.
**Files (data: URI path):** Modify `src/infrastructure/files/src/filesystem.rs`.

**This task's first step is the decision itself, not an assumption:** if Task 12's spike
held, build the route (below). If it did not, and Ross has made a revised architecture
decision, follow that decision. The `data:` URI path is only taken if it is separately
measured (actual IPC-payload size and decode time for the images actually in use) to have
no material cost, and that measurement is written down as the reason — never adopted by
default. Neither path introduces a new byte/pixel cap, truncation, or downscale.

**If building the capability route — acceptance criteria:**
1. On a `file-viewer.read` response whose `file_type` is `image`, `bridge.rs`'s closure
   (which already has `webview_clone` in scope) opens the canonical path from the response
   ONCE, keeps the open file handle, and mints a grant recording `(open handle,
   owner: webview_clone.clone())` in a store local to `quantum-ui`. It rewrites the
   response's `uri` to `quantum://viewer-image/<opaque-token>` (CSPRNG-backed, unguessable)
   before it reaches the webview. `application`'s `FilesService`/dispatcher is untouched —
   this is a `ui`-layer interception of an already-complete response.
2. `scheme.rs`'s handler, for the `viewer-image` route, looks up the token, calls
   `request.web_view()`, and serves bytes read from the grant's open handle only when the
   returned `WebView` is identity-equal to the grant's `owner`. A mismatch, `None`, or an
   unknown token all serve not-found — nothing distinguishes "wrong viewer" from "unknown
   token" in the response, so neither leaks information about the other.
3. `registry.rs`'s window teardown (`destroy_window`, and `hide_window` for a view that
   stays warm) revokes every grant owned by that window's `WebView` identity.
4. The image extension of the originally-opened path may be checked as a sanity filter, but
   ownership (the `WebView` identity match) is the only thing that admits a serve — an
   extension match alone never does.
5. No root allowlist, no path segment in the route (only the opaque token).

**Required adversarial tests (all red before green), in `scheme.rs`'s test module unless
noted:**
```rust
#[test]
fn resolve_rejects_an_unissued_token() { /* syntactically plausible, never issued: not-found */ }

#[test]
fn resolve_rejects_a_grant_requested_by_a_different_webview() {
    // issue a grant owned by webview A; resolve the same token while asserting the
    // request's web_view() is webview B; must be rejected, not served
}

#[test]
fn resolve_rejects_a_grant_after_its_owning_window_closes() {
    // issue a grant, simulate the owning window's destroy_window/hide_window path,
    // then resolve; must be rejected
}

#[test]
fn resolve_serves_the_original_bytes_even_if_the_path_is_later_replaced_with_a_symlink() {
    // issue a grant for a real file (opening its handle), replace the file's path with a
    // symlink to different content, then resolve — must still return the ORIGINAL bytes
    // (proving the open-handle design, not a path re-open, is what actually served)
}

#[test]
fn an_image_extension_alone_never_admits_a_serve_without_a_matching_owner() {
    // a request whose token maps to a real, correctly-extensioned path but whose
    // web_view() does not match the owner must still be rejected
}
```

**If building the measured `data:` URI alternative instead — acceptance criteria:**
1. The measurement from Task 12's decision gate (or a dedicated measurement pass) is
   recorded: representative image sizes actually in use, resulting IPC payload size, and
   observed decode/render time, with the conclusion that none of this is material.
2. `read_for_viewer_blocking`'s `ViewerFileType::Image` arm returns
   `data:{mime_type};base64,{encoded}` instead of `file://...`, with no byte cap — every
   readable image is inlined in full; an unreadable file is a typed error, not a cap error.
3. The existing `read_for_viewer_returns_image_with_uri` test (`filesystem.rs:1191-1216`)
   is updated in the same change to assert the `data:` prefix instead of `file://`.

**Steps (either path):** write the adversarial/contract tests first, confirm they fail for
the right reason, implement, re-run until green, then run the full crate suite for every
file touched (`./scripts/devsh.sh cargo test -p quantum-ui` and/or
`-p quantum-files`) to confirm no pre-existing test (including the icon-route tests in
`scheme.rs`) regressed.

---

### Task 14: Zoom, fit, actual-size, and pan in `ImageRenderer`

**Files:** Modify `ImageRenderer.svelte`. Create `ImageRenderer.test.ts`.

**Acceptance criteria:**
1. Explicit zoom state with `+`/`-` controls, each step bounded (exact bounds decided
   during implementation).
2. A "Fit" mode restoring today's default behavior as an explicit, named state.
3. An "Actual size" mode: literal 1:1 natural pixel dimensions, stated explicitly in code.
4. Pannable (drag-to-scroll) whenever the image exceeds the viewport at the current zoom;
   aspect ratio never distorts at any zoom level.
5. A decode/resource-load error renders a visible, actionable message (not a blank pane),
   mirroring `App.svelte`'s existing error-state styling.

This task is independent of Task 13's transport: `ImageRenderer` only ever receives a
`uri` string prop and never inspects its scheme.

**Step 1 (red)** — illustrative subset:
```ts
const SAMPLE_URI = 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=';

test('Fit is the default mode', () => {
    const { container } = render(ImageRenderer, { props: { uri: SAMPLE_URI, filename: 'x.png' } });
    expect(container.querySelector('.mode-fit')?.classList.contains('active')).toBe(true);
});
test('a load error renders a visible message, not a blank pane', async () => {
    const { container, getByAltText } = render(ImageRenderer, { props: { uri: 'data:image/png;base64,invalid', filename: 'bad.png' } });
    await fireEvent.error(getByAltText('bad.png'));
    expect(container.querySelector('.image-error')).not.toBeNull();
});
```
Run `pnpm exec vitest run src/lib/ImageRenderer.test.ts` — FAIL (no controls exist yet).
**Step 2:** implement zoom/fit/actual-size/pan state and the error handler, plus the
remaining acceptance-criteria tests — PASS.

---

### Task 15: Format honesty — verify SVG, BMP, ICO, AVIF against the pinned WebKitGTK

Verification task; no code changes unless a gap is found. For each of `svg`, `bmp`, `ico`,
`avif` (all currently `ViewerFileType::Image` per `file_viewer.rs:72-74`), a live check
(coordinate with Ross) opens a real file of that format via `qv` and records whether
WebKitGTK 6 (this repo's pinned version) decodes and displays it through whichever
transport Task 13 built. Any format that fails gets Task 14's visible error state — it is
NOT silently dropped from the extension list without taking the finding back to Ross first.

---

### Task 16: Live verification — zoom/pan/fit feel and special-character paths

Manual, live check (coordinate with Ross). Open images with a space and a non-ASCII
character in the filename via `qv`; confirm the load still succeeds and that zoom/fit/
actual-size/pan all behave and aspect ratio never distorts. If this verification or Task
15's format sweep surfaces a genuine problem (not a size limit — none exists — but an
actual failure), record the finding and take it to Ross rather than inventing a workaround.

---

## Hand-off: commits, release notes, and deployment

**No commit, push, or merge in this plan is authorized yet.** Every task ends with local
tests green; stop there. When Ross explicitly authorizes committing this work:

- Commit per task, conventional-commit style (`feat:`, `fix:`, `test:`), per `AGENTS.md`.
- Write a short release note per shipped item (listing fix, `qv` search, `qv` image
  zoom/fix) before asking Ross to test it live, per his standing preference.
- Do not mark the originating inbox item(s) done until verified on the installed binary,
  not just the dev build — per `AGENTS.md`'s documented dev-build-versus-service-binary
  distinction.
- Nix packaging (flake bump, `just build`) and activation (`just switch`) follow the root
  exocortex `AGENTS.md` deploy procedure: the agent takes the change up to a built,
  evaluated configuration; Ross alone runs `just switch`.
- Leave the pre-existing dirty, unrelated files (power-menu/sound-menu/panel) untouched
  throughout every task in this plan.
