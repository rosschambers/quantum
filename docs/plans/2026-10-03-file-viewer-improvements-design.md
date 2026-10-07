# Quantum File Viewer and Explorer Improvements — Design

Companion implementation plan: `docs/plans/2026-10-03-file-viewer-improvements.md`.

This records the initial investigation against `a6903a68`. For implemented behavior,
verification evidence, and integration status, read the implementation plan's
**Execution status** section rather than treating these baseline findings as current bugs.

Three independent problem areas, approved by Ross:

1. The files explorer (`plugin/files/files`) is unresponsive on large directories.
2. The file viewer (`plugin/file-viewer/file-viewer`, opened via `qv <path>`) has no
   in-file search.
3. The file viewer's image rendering has no zoom and an unverified load failure.

Each area below states what is **confirmed by reading the current source** (with exact
file and line references, re-verified against this checkout — the paths quoted in the
preliminary investigation report omitted the `src/` segment for the `files` view's
`App.svelte`; the correct path is `src/ui/plugins/files/views/files/src/App.svelte`,
confirmed below), what is **still unproven**, and the design for closing the gap. Nothing
here claims a root cause is proven unless a reproduction is cited; where only source
inspection supports a hypothesis, it is marked as such.

## Area 1 — Explorer directory-listing responsiveness

### Confirmed by source inspection

**The explorer issues up to three identical full `files.list` calls for the same path on
a single open**, independent of the `quantum-files` backend's own per-entry cost:

- `src/ui/plugins/files/views/files/src/App.svelte:277-332` (the startup `$effect`): when
  opened with `window.__quantum_args.path` set (dual-pane is the default — `dualPane =
  $state(true)` at line 83), it first calls `await ipc.list(argsPath)` purely to validate
  the path is listable (line 300), discards the result, then calls
  `panes[0].navigate(argsPath)` and `panes[1].navigate(argsPath)` (lines 301-302).
- `src/ui/plugins/files/views/files/src/App.svelte:387-407` (`setupPaneLoader`): each pane
  has its own `$effect` keyed on `pane.path` that calls `loadPane(pane)` → `ipc.list(pane.path)`
  (line 231) whenever `path` changes. `navigate()` sets `path` (`paneState.svelte.ts:77-85`)
  but performs no IPC itself — the effect is what fires the real listing.

So opening the explorer at a path via `window.__quantum_args` fires: one throwaway
validation listing, then one real listing per pane — three full directory walks for one
visible result, with dual-pane the default.

- `src/application/src/use_cases/files_service.rs:138-140` — `FilesService::list` is a bare
  pass-through (`self.filesystem.list_directory(path).await`) with **no in-flight request
  sharing and no cache**, so all three calls above hit the real filesystem independently.
  The service already has a proven pattern for sharing concurrent per-path work — the
  reference-counted `Subscription` used by `watch_handles` and `size_handles`
  (`files_service.rs:67-134`, with generation-stamped self-removal on completion) — it is
  just not applied to `list`.

**Each of those three listings pays an identical, avoidable per-entry cost.**
`list_directory_blocking` (`src/infrastructure/files/src/filesystem.rs:148-164`) calls
`entry_from_path` once per directory entry (line 161); `entry_from_path`
(`filesystem.rs:98-143`) calls `resolve_owner(uid)` (line 138) for every entry;
`resolve_owner` (`filesystem.rs:63-68`) calls `owner_from_passwd(uid)`
(`filesystem.rs:72-93`), which **reads the entire `/etc/passwd` file from disk and
linearly scans it from the top on every single call** — there is no caching across
entries within one listing, let alone across the three duplicate listings above. The
directory walk itself already runs on Tokio's blocking pool (`run_blocking`,
`filesystem.rs:672-690`, used by the `FileSystemPort::list_directory` impl at
`filesystem.rs:687-690`), so the walk does not block the async runtime, but the
`/etc/passwd` cost inside it is still real CPU and I/O work, multiplied by three on open.

### What is NOT yet proven

The prior investigation's timing numbers (`/tmp`, 7345 entries, 218–253 ms;
`/etc`, 101 entries, 21–22 ms) were taken by driving the `quantum-files`
backend crate directly from CLI/test code — **not** from the installed daemon's live
WebKit-hosted explorer window, and not broken down by phase. They show the per-listing
backend cost scales with entry count, consistent with the `/etc/passwd`-per-entry read
above, but they do not by themselves prove that this backend cost — as opposed to IPC
JSON serialization/transfer, or `FileList.svelte`'s sort/render — is what the user
perceives as a freeze, nor that the currently-installed binary matches this checkout's
`HEAD`. Per the approved scope, the three phases (listing, IPC transfer, sort/render) must
be measured separately against the real live path (thousands of entries, no truncation)
before any claim that the backend fix alone resolves the perceived freeze.

**Note:** this is an explorer-only bug. The file viewer (`qv`) reads a single file via
`file-viewer.read` (`App.svelte:51` in the file-viewer plugin) and never lists a directory;
it is unaffected by anything in this section.

### Design

1. **Coalesce concurrent same-path `list` requests in `FilesService`, with no persistent
   cache.** Add an in-flight map (`Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>` or
   equivalent — see the plan for the exact shape) keyed by path: a request for a path
   already in flight awaits the existing call's result instead of starting a new
   filesystem walk; once the walk settles (success or error), its entry is removed
   immediately — no entry survives past the request(s) that triggered it, so a later,
   independent open of the same path always re-lists from disk. This directly answers
   "share simultaneous requests for same path with settled entries removed" and fixes the
   triplicate-call case above without touching the three call sites in `App.svelte` (the
   validation call and both panes' loaders still each *ask*, but only the first actually
   walks the filesystem).
2. **Parse `/etc/passwd` once per listing, not once per entry.** Split `owner_from_passwd`
   into a pure parser (`parse_passwd(contents: &str) -> HashMap<u32, String>`, independently
   unit-testable with a crafted fixture string) and a loader that reads the file once per
   `list_directory_blocking` call; thread the resulting map into `entry_from_path` as an
   optional borrowed lookup so the single-entry `stat` port method (which also calls
   `entry_from_path`) is unaffected and keeps its own simpler per-call read. This is a
   constant-factor win (one file read instead of N) on top of the coalescing fix, and
   applies even to a single, non-duplicated listing.
3. **Preserve the existing virtualized list.** `FileList.svelte` already virtualizes rows
   (`ROW_HEIGHT = 30`, `scrollTop`-driven `startIndex`/`offsetY` arithmetic,
   `src/ui/plugins/files/views/files/src/lib/FileList.svelte:89-127`) — this is not a gap.
   Incremental/paginated loading of entries is explicitly **out of scope** unless the
   separated measurements in the plan's verification task show the full-listing IPC
   payload itself (not virtualization, not sort) is the dominant cost on a real
   multi-thousand-entry directory; if so, that is a larger contract change (streaming or
   paged `files.list`) requiring its own design decision with Ross before implementation,
   not something to improvise inside this plan.

## Area 2 — qv in-file search (Ctrl+F)

### Confirmed by source inspection

- The file viewer has no search today: `App.svelte` (file-viewer,
  `src/ui/plugins/file-viewer/views/file-viewer/src/App.svelte:60-82`) only handles
  Ctrl+ArrowUp/Down (markdown heading nav) and Escape (close) in its `onMount`-installed
  `keydown` listener. There is no `viewerKeymap.ts`-equivalent pure resolver in this plugin
  today; the explorer's `src/ui/plugins/files/views/files/src/lib/keymap.ts` (a pure
  `(event: KeyboardEvent) => ShortcutAction | null` resolver, unit-tested in
  `keymap.test.ts`) is the pattern to mirror, per the approved design.
- **Virtualization and folding remove offscreen/collapsed content from the render
  entirely — they do not just hide it with CSS** — so a naive DOM or string search over
  the live rendered output will miss matches that are off-screen or folded:
  - `CodeRenderer.svelte:23` and `TextRenderer.svelte:12` both switch to
    `VirtualScroller` when `lineCount > 500`; `VirtualScroller.svelte:31`
    (`visibleLines = lines.slice(visibleStart, visibleEnd)`) renders only the lines
    within the scrolled viewport plus a 50-line buffer. There is currently no imperative
    way to scroll it to an arbitrary line from outside — `scrollTop` is internal
    component state set only by its own `scroll` listener.
  - `CodeRenderer.svelte`'s non-virtual path (`<= 500` lines) and `JsonFoldRenderer.svelte`
    (which has **no virtualization threshold at all** — it always renders every line,
    folded or not) both build a `*VisibleLine[]` array that **skips every line inside a
    collapsed fold** (`CodeRenderer.svelte:53-60`, `JsonFoldRenderer.svelte:102-111`): a
    match on a collapsed line is not in the DOM to find.
  - **Important asymmetry:** when `useVirtualScrolling` is true, `CodeRenderer.svelte`'s
    virtual-scroll branch (lines 82-91) renders `props.visibleLines` directly from the raw
    `lines` array and does **not** apply `codeFoldModel`/`codeFoldState` at all — so for
    code/text files over 500 lines, there is currently no folding to work around in the
    first place; only the >500-line virtualization windowing applies. Folding only matters
    for JSON (always) and for code/text files of 500 lines or fewer.
  - `MarkdownRenderer.svelte` renders one continuous HTML blob from `marked` — there is no
    virtualization and no folding, so a markdown search only needs to scroll the match
    into view, never to force-expand anything.
- Mermaid/Graphviz diagram fences render into placeholder `div.diagram-block` elements,
  created by `renderer.code`'s `diagramPlaceholderHtml` calls
  (`MarkdownRenderer.svelte:78-89`; markup confirmed by the existing
  `mermaid.test.ts:34-39`, the only component-adjacent test currently in this plugin). The
  approved scope excludes diagram SVG/source from search — the DOM walk must skip these
  nodes.

### Design

- **`viewerKeymap.ts`** (new, file-viewer plugin): pure resolver mirroring
  `files/.../keymap.ts`'s shape — `resolveViewerShortcut(event): ViewerShortcutAction |
  null` returning `{ kind: 'open-search' }` (Ctrl/Cmd+F), `{ kind: 'next-match' }` /
  `{ kind: 'previous-match' }` (Enter / Shift+Enter, only meaningful while search is open —
  resolved by the caller's state, not by this pure function), and `{ kind: 'close-search' }`
  (Escape — see the ordering note below). Unit-tested the same way as `keymap.test.ts`.
- **Escape ordering:** today Escape always closes the viewer window
  (`App.svelte:79-81`). The approved design requires Escape to close an open search bar
  **first**, and only close the viewer on a second Escape (or on Escape when search is
  already closed). This is a stateful decision (is search open?), so it is resolved in
  `App.svelte`'s handler, not inside the pure keymap resolver — the resolver only reports
  "Escape was pressed"; the caller decides what that means given current UI state.
- **Match model:** a pure function, `findMatches(text: string, query: string):
  MatchRange[]` (`{ start: number; end: number }` byte/char offsets into `text`),
  literal case-insensitive substring search — no regex, no replace, matching the approved
  scope. One module per content type's notion of "text":
  - Text/Code: matches computed per logical line against the RAW (pre-highlight) line
    string from `lines[i]`, so offsets never have to account for injected `<span>` markup.
  - JSON: same, against `lines[i]` (the pretty-printed JSON source line), consistent with
    how `JsonFoldRenderer` already keys everything by line index.
  - Markdown: matches are found by walking the RENDERED DOM's text nodes (a `TreeWalker`
    over `markdownContentElement`, skipping subtrees inside `.diagram-block` /
    `.diagram-error`), not the raw markdown source — the approved scope is explicit that
    markdown search means "visible rendered text, not raw formatting." This is the
    one content type where matching operates on the DOM rather than on a string the
    renderer owns, because marked's HTML output is the only form "visible text" exists in.
- **Highlighting without corrupting syntax-highlighted HTML.** `CodeRenderer` and
  `JsonFoldRenderer` currently call `highlightCode(line, language)` once per whole line and
  inject the result via `{@html ...}`. Splicing `<mark>` tags into that HTML string by
  naive string search would risk landing inside an existing `<span>` and breaking the
  markup. The design instead **highlights per matched segment**: when a line has matches,
  split the line's plain text into `{ text, isMatch }` segments at the match boundaries,
  run `highlightCode` independently on each segment, and wrap `isMatch` segments in
  `<mark class="search-match">`. This is a deliberate, documented trade-off (consistent
  with this project's style of calling out accepted limitations rather than hiding them):
  a match that falls inside a multi-character token (for example mid-string-literal) may
  highlight that token's syntax color slightly differently right at the match boundary,
  because `highlightCode` loses the surrounding-line context for that segment. This is
  cosmetic only and does not affect match correctness. No such trade-off exists for
  Markdown (DOM-level `<mark>` insertion around existing text nodes, which never touches
  already-rendered syntax-highlighted `<span>` elements because code fences inside
  rendered markdown are highlighted once at parse time, not per keystroke).
- **Revealing offscreen/collapsed matches:**
  - `VirtualScroller.svelte` gains a way to be told to scroll to a specific line index from
    outside (for example a reactive `scrollToIndex` prop watched by an internal `$effect`
    that sets `container.scrollTop = index * lineHeight`, clamped to the scrollable range).
    This is the only change needed to make virtualized Text/Code search-navigable.
  - For a match inside a currently-collapsed fold (JSON always; Code/Text only when
    `lineCount <= 500`), the design is a lookup of "which fold (if any) currently collapses
    this line" against the existing `codeFoldModel`/`foldModel` (`fold-model.ts`'s
    `CodeFoldRange`, and `JsonFoldRenderer`'s inline `FoldRange`), then a single
    `foldState.set(thatFold.startLine, false)` — expanding exactly the minimal containing
    fold, leaving every sibling and ancestor fold's state untouched, and leaving it expanded
    after the search closes (the approved scope is explicit: "permanently expand... preserve
    others"). No existing fold-model code currently supports "find the fold containing line
    N" as a query — this is a small new pure function (`foldContaining(model, line):
    CodeFoldRange | null`), not a redesign.
- **Search bar UI:** a small bar mounted in `App.svelte`'s `content-area`, following the
  explorer's established component style (plain Svelte component, theme tokens only, no
  new CSS variables) — exact placement and visuals are a plan-time styling detail, not a
  design decision requiring sign-off.
- **Testability risk, flagged for the plan to resolve before writing integration tests:**
  file-viewer's `App.svelte` registers its only keyboard listener inside `onMount`
  (`App.svelte:40,84`). Quantum's own project convention
  (`AGENTS.md`: *"Svelte 5 components used in vitest must use `$effect` for setup work,
  not `onMount`. testing-library's legacy adapter does not fire `onMount` reliably under
  Svelte 5 runes mode."*) means a vitest-rendered `App` may never attach this listener,
  which would make any Ctrl+F integration test pass or fail for the wrong reason (a false
  green if the test also never dispatches a real event, or a permanently-red test that
  looks like a feature bug but is actually a test-harness gap). The explorer's own
  `files/.../App.svelte` and this plugin's own `Header.svelte`/`ImageRenderer.svelte` use
  `onMount` too, but the explorer's keyboard wiring is NOT inside `onMount` — its shortcuts
  flow through `window.addEventListener` set up differently (not yet confirmed to be
  outside `onMount` here; this needs a one-time empirical check, not an assumption). The
  plan's first search task must spike this empirically (render `App` under
  `@testing-library/svelte/svelte5`, the confirmed-working adapter for Svelte 5 in this
  repo — see `files/.../Toolbar.test.ts:2`, both plugins' `vite.config.ts` already set
  `environment: "jsdom"` and both package.json files already list `@testing-library/svelte`,
  `jsdom`, and `vitest` as devDependencies, so no new dependency is needed) and confirm
  whether the keydown listener attaches. If it does not, moving the listener registration
  from `onMount` into a top-level `$effect` is a small, in-scope prerequisite refactor
  (behavior-preserving) before adding the search feature on top of it.

## Area 3 — qv image viewer (zoom/pan, load-failure reproduction)

### Confirmed by source inspection

- **No zoom today.** `ImageRenderer.svelte` is 29 lines total: an `<img>` with
  `object-fit: contain; max-width: 100%; max-height: 100%` and nothing else — no state, no
  controls (`src/ui/plugins/file-viewer/views/file-viewer/src/lib/ImageRenderer.svelte:1-29`).
- **The standalone image path serves a raw `file://` URI.**
  `read_for_viewer_blocking` (`src/infrastructure/files/src/filesystem.rs:539-668`) returns
  `uri: Some(format!("file://{canonical_path_str}"))` for `ViewerFileType::Image |
  ViewerFileType::Video` (lines 592-606); `App.svelte` (file-viewer) passes that straight
  into `ImageRenderer`'s `uri` prop whenever `fileInfo.file_type === 'image'`
  (`App.svelte:125-126`), which lands in a plain `<img src>`
  (`ImageRenderer.svelte:11`). There is an existing passing unit test asserting exactly
  this contract — `read_for_viewer_returns_image_with_uri`
  (`filesystem.rs:1191-1216`) asserts `info.uri` `starts_with("file://")` — so any change
  to this URI scheme (see design below) must update that test as part of the same change,
  not as an afterthought.
- **Corrected claim: `embed_markdown_images` is qv's own code, not the explorer's, and
  markdown images are mostly already safe.** The previous draft of this design document
  was wrong on this point. `embed_markdown_images` (`filesystem.rs:461-534`) has exactly
  one call site in the entire crate (confirmed by searching the crate): `filesystem.rs:636`,
  inside `read_for_viewer_blocking` itself, in the `ViewerFileType::Markdown` arm — this
  **is** `qv`'s own backend function, not a sibling explorer-preview code path. For a
  **relative** image reference in markdown source (`is_relative_image_path`,
  `filesystem.rs:419-426`) that is readable, has a recognized image MIME type, and is at
  most `MARKDOWN_IMAGE_EMBED_MAX_BYTES` (5 MiB, `filesystem.rs:417`),
  `read_file_as_data_uri` (`filesystem.rs:428-450`) already rewrites it to a base64
  `data:` URI **before** the content ever reaches `MarkdownRenderer.svelte` — so the common
  case of a small relative markdown image already renders safely today, with no `file://`
  involved at all and no cross-origin concern. Only two narrower cases still reach
  `MarkdownRenderer.svelte`'s `resolveImageSrc` (`MarkdownRenderer.svelte:43-48`) with an
  unrewritten href, which it then resolves to `file://${fileDirectory}/${href}`
  (line 47): **(a)** an absolute-path image reference (`is_relative_image_path` only
  rewrites relative ones), and **(b)** a relative reference whose target is unreadable,
  not image-MIME, or exceeds the 5 MiB cap (`read_file_as_data_uri` returns `None`, so
  `embed_markdown_images` leaves the original href untouched, `filesystem.rs:500-510`).
  Per the approved scope, **this plan does not touch markdown image handling at all** —
  neither the already-safe common case nor these two edge cases — unless the live
  reproduction below finds a concrete, directly-related bug in them, which would then be
  its own separately-scoped decision, not bundled into the standalone-image fix.
- **The standalone image view has no equivalent safety net.** Unlike markdown's
  `embed_markdown_images`, the `ViewerFileType::Image` arm of `read_for_viewer_blocking`
  (lines 592-606) does no data-URI rewriting at all — it always returns a raw `file://`
  URI. Whether this actually fails to load under WebKit (the concern that motivated
  markdown's data-URI workaround) is suspected, not proven, for this specific call path —
  the plan's first image task is to actually load an image through `qv` in the real
  installed WebKit webview and observe whether it renders or fails, and capture how
  (blank image, broken image icon, console/network error) before designing the fix around
  that specific failure mode.
- **Existing security precedent for serving an absolute filesystem path through
  `quantum://` safely** exists for icons: `quantum://icon/<percent-encoded-path>`
  (`src/ui/host/src/scheme.rs:110-116, 181-186`) is parsed by `parse_quantum_uri`, then
  validated and served by `read_icon_file_from`
  (`scheme.rs:300-319`): requires a recognized image extension
  (`is_image_extension`, lines 220-230), canonicalizes the requested file's *parent
  directory*, and requires that directory to sit under a fixed, environment-derived
  allowlist of icon root directories (`allowed_icon_roots`, lines 268-284). **This
  precedent does not transfer to a user-opened image file**, and must not be copied as-is:
  icon roots are a fixed, non-secret, system-wide location (`/usr/share/icons` and
  similar) — there is no equivalent allowlist directory for "any image the user's own
  account can read," so an extension-plus-canonicalization-only route at a path segment
  that embeds the real path (`quantum://viewer-image/<percent-encoded-path>`) would be
  reachable by **any** rendered content that can embed an `<img src>` — including a
  crafted third-party markdown file rendered in this same plugin, or any other page served
  under the shared `quantum://` scheme — letting it probe or load **any image file the
  daemon's own process can read**, never opened by the user through `qv` at all. This is
  the mistake the previous draft of this document made in recommending exactly that
  approach ("Option A"). The icon route's narrowness (one scheme, one trust boundary: files
  the SYSTEM ships, not files the USER owns) is precisely why it is safe; a viewer-image
  route serving arbitrary user files by canonical path is a categorically different, far
  more sensitive endpoint and needs a different authorization model entirely (see Design,
  below). There is no decision for Ross here: an exact-resource, capability-scoped route is
  the only acceptable shape if a route is built at all — that is already a settled
  constraint, not an open choice.
- **Format support claims, checked against the actual domain mapping.**
  `viewer_file_type_for_extension` (`src/domain/src/file_viewer.rs:72-74`) currently
  classifies `png | jpg | jpeg | gif | webp | svg | bmp | ico | avif` as
  `ViewerFileType::Image` — all nine are routed to `ImageRenderer` today regardless of
  whether the browser engine can actually decode them. The approved scope requires
  testing each advertised format honestly rather than silently dropping support for any of
  them without an explicit decision; WebKitGTK's native `<img>` decoding support for
  `avif`/`ico` in this project's pinned WebKitGTK 6 version is not yet verified and must be
  checked, not assumed, during implementation.

### Design

- **Reproduce before fixing.** The first image task is a live, installed-WebKit
  reproduction: open a known-good PNG through `qv` on the real daemon and record what
  actually happens (renders correctly / blank / broken-image icon / console error). No
  assumption that `embed_markdown_images`'s workaround need applies to this different call
  path — this call path (the standalone image view) is read-for-viewer's own, separate
  `ViewerFileType::Image` arm, with no data-URI rewriting today.
- **Two firm constraints on any fix, not open questions:** no new byte/pixel cap,
  truncation, or downscale on the standalone view beyond what exists today, and no
  path-based route under any framing (`quantum://viewer-image/<the-real-path-in-some-
  encoding>` is insecure regardless of serve-time validation — the real path is *in the
  request itself*, so anything that can embed an `<img src>` under the shared `quantum://`
  scheme can request any path it knows or guesses, whether or not the user opened it with
  `qv` — an image extension on the requested segment is never sufficient admission on its
  own). If a real need for a size limit ever surfaces, it is measured first and decided by
  Ross — never defaulted by this plan.
- **Preferred fix: an exact-resource, WebView-identity-bound capability route —
  architecture-spike first, before building it.** Verified against the actual host and the
  pinned WebKit binding (not assumed):
  - **Issuing needs no new cross-layer plumbing.** `bridge.rs`'s script-message-received
    closure (`bridge.rs:56-58,97`) already holds `webview_clone: WebView` — the exact
    WebView that sent the `file-viewer.read` call — in scope at the point it calls the
    dispatcher. A grant bound to that WebView's own identity can be minted right there, in
    `quantum-ui`, without touching `application`'s dispatcher or the IPC wire format: the
    whole lifecycle (issue, resolve, revoke) can live inside `quantum-ui`, where
    `bridge.rs`, `scheme.rs`, and `registry.rs` already sit side by side.
  - **Resolving can check real request provenance.** `webkit6::URISchemeRequest` exposes
    `pub fn web_view(&self) -> Option<WebView>` — confirmed present in this repo's exact
    pinned version (`webkit6 0.4.0` per `Cargo.lock`; verified against that version's own
    published API, not a later release). The scheme handler can call this on the incoming
    request and compare it, by GObject identity (the same category of pointer-identity
    comparison `registry.rs` already uses for `MonitorId`), against the WebView recorded
    with the grant at issue time. A mismatch, or `None`, is rejected. This makes "another
    viewer cannot load this grant" an enforced, structural property — not an assumption
    resting on "nobody else was ever told the token."
  - **Revocation on window close is directly available, for the same reason.**
    `WindowRegistry::destroy_window` / `hide_window` (`registry.rs:644,666`) sit in the same
    crate as the grant store would, so closing a view's window can revoke its grants
    directly. No TTL and no single-resolve fallback is needed as the primary guarantee.
  - **This is not yet proven end-to-end — confirmed only in the API surface, not in this
    daemon's real behavior.** Whether `web_view()` reliably returns the correct originating
    WebView for a genuine `<img src>` sub-resource load inside this daemon's actual
    webviews has not been empirically checked here. The plan's architecture-spike task
    checks this live, before anything is built on top of it. **If the spike finds this does
    not hold, stop and bring the finding back to Ross for a revised architecture
    decision — do not fall back to a weaker token-only or TTL-only scheme as a silent
    substitute;** that would be exactly the insecure downgrade the approved design rules
    out.
  - **Open-at-issue-time, not re-stat-at-resolve-time.** The grant holds an open file
    handle, obtained by opening the canonicalized path once when the grant is issued, and
    resolving serves bytes read from that handle — not a path string re-opened later. This
    structurally eliminates the TOCTOU symlink-replacement race (a path swapped after issue
    cannot change what an already-open descriptor reads), which is a stronger guarantee
    than re-canonicalizing a path string at resolve time.
  - **Ownership is the only admission criterion.** The WebView-identity check is what gates
    serving; extension and regular-file checks remain only as defense-in-depth sanity
    checks, never as a substitute for, or alternative path around, the ownership check.
- **Alternative: a `data:` URI — only if measured, documented, and still uncapped.**
  Inlining the image bytes directly (the same technique `embed_markdown_images` already
  uses) needs no route or grant system at all, since it is not a named resource any other
  content can address. It is a valid simpler choice, but only adopted if the plan's
  live-verification work actually measures no material IPC-payload or decode-time problem
  across the images in real use, and the choice is written down with the numbers that
  justified it — not adopted by default, and not paired with inventing any byte/pixel cap
  or truncation: an image is inlined in full or the route is used instead; it is never
  silently shrunk.
- **Zoom, fit, actual size, pan.** `ImageRenderer.svelte` gains explicit state: a zoom
  level with `+`/`-` controls, a "Fit" mode (today's default `object-fit: contain`
  behavior, restated as an explicit mode rather than the implicit-only default), and an
  "Actual size" mode that sets CSS to literal `100%`/`1:1` natural pixel dimensions
  (explicitly, not inferred) as its own named mode distinct from "Fit." At any zoom level
  above "Fit," the image must remain pannable (drag to scroll within the viewport) and must
  never distort its aspect ratio (width/height scale together, never independently). This
  work is independent of which transport (the capability route or, if the evidence
  supports it, a data: URI) supplies the `uri` prop — `ImageRenderer` only ever sees a
  string and does not need to know which.
- **Error handling stays honest.** A resource load failure (404-equivalent, decode
  failure) or an unsupported/undecodable format must surface a visible, actionable error
  state in `ImageRenderer`, never a silently blank pane. No size cap, truncation, or
  downscale is introduced by this plan for the standalone view under either transport. The
  existing `MARKDOWN_IMAGE_EMBED_MAX_BYTES` (5 MiB, `filesystem.rs:417`) and
  `IMAGE_PREVIEW_MAX_DIMENSION` (512px, `files_service.rs:36`) are unrelated existing
  constants belonging to markdown embedding and the *explorer's* preview thumbnail
  respectively — neither is reused, nor treated as precedent, for qv's standalone view.

## Verification requirements (all three areas)

- Root-cause claims must be backed by an actual reproduction (a failing test, or an
  observed failure in the real installed WebKit webview), never inferred solely from a
  backend micro-benchmark or a code comment about a different call path.
- New or changed behavior gets a failing test first, then the minimal change that turns it
  green (see the companion plan for each task's concrete red/green steps).
- Any check that requires the live installed daemon (image load reproduction, zoom/pan feel,
  large-directory responsiveness) is a headless/isolated check first wherever one exists
  (unit/vitest), and the live/installed check is listed as its own explicit, separate step —
  it is never silently assumed to follow from the headless check passing.
- No GTK windows are opened by the agent without Ross's explicit permission in the moment;
  no polling loops against a live desktop session.
- Nix packaging/deployment (flake bump, `just build`, and eventually `just switch`) is out
  of scope for implementation until Ross explicitly authorizes commits for this work; see
  the companion plan's final section.

## Explicit non-goals

- Redesigning the explorer's virtualized list or introducing paginated/incremental
  directory loading, unless the plan's own measurement task proves it necessary.
- Regex or replace-in-file search in `qv`.
- Expanding `qv`'s Markdown search to diagram SVG source or any raw-markup view.
- Any change to `files.preview` (the explorer's own preview pipeline) — the image-loading
  fix in Area 3 is scoped to the standalone image view (`ViewerFileType::Image`) only.
- Any change to `qv`'s own markdown image handling (`embed_markdown_images`,
  `MarkdownRenderer.svelte`'s `resolveImageSrc`) — confirmed already safe for the common
  case (relative, readable, ≤5 MiB images are already rewritten to `data:` URIs before
  `MarkdownRenderer.svelte` ever sees them) and explicitly left alone for its two narrower
  edge cases (absolute-path references; oversized/unreadable relative references) unless
  the live reproduction in Area 3 finds a concrete, directly-related bug in them — which
  would then be its own separately-scoped decision, not folded into this plan.
- Building the capability-grant route (or the alternative data: URI) without first running
  the architecture spike (see Area 3, Design) — if the spike finds WebView-identity binding
  does not hold in practice, building anything in this area stops for a revised
  architecture decision from Ross, rather than proceeding on a weaker model.
- Inventing any new byte/pixel cap, truncation, or downscale for the standalone image view,
  under either transport, without a measured case taken to Ross first.
