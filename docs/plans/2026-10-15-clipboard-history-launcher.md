# Clipboard history in the launcher — implementation plan

> **For OpenCode:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Super+V opens quantum's launcher straight into clipboard mode with a side-by-side preview of the
highlighted copy, quantum stops recording copies marked sensitive, and the two leftover clipboard recorders
(cliphist and walker/elephant) are removed.

**Architecture:** No new IPC. Super+V calls `quantumctl show launcher --args '{"query":";"}'`. The host already
injects `window.__quantum_args` on every open, as `null` when there are no arguments, and the launcher starts
from that query. The preview renders the full text that each clipboard text result already carries in its
primary `copy` action. The watcher drops any change whose offered MIME types include
`x-kde-passwordManagerHint`.

**Tech Stack:** Rust (`quantum-providers`), Svelte 5 + vitest (`default-launcher` view), NixOS/Home Manager
(config repo), Hyprland.

**Design:** `docs/plans/2026-10-15-clipboard-history-launcher-design.md`. The layout was settled in
`docs/playgrounds/2026-10-15-clipboard-preview-layout.html`.

**Assumptions:**
- Quantum work happens on `main` in the canonical checkout `code/projects/quantum`, as Ross prefers for
  routine work. The repository is **public**: no real clipboard contents and no secrets in fixtures, tests or
  commits.
- All quantum commands run through `./scripts/devsh.sh` from the repository root. Allow at least 10 minutes
  for any cargo command.
- Launcher vitest runs from the view directory:
  `./scripts/devsh.sh bash -c "cd src/ui/plugins/launcher/views/launcher && pnpm exec vitest run"`.
- The clipboard provider id is `clipboard` (`providersForQuery(';')` returns `['clipboard']`). Text entries'
  `match.action` is `{ kind: 'copy', data: { text } }` (`src/infrastructure/providers/src/clipboard.rs:141`).
  Image entries carry a `data_uri` icon thumbnail and a `custom` recopy action. `text/uri-list` copies are
  stored as text entries.
- The config repo's `just build` runs `git add .` first (`pre-build`), so start every config task from a
  clean tree and commit by explicit path.
- The agent never runs `just switch`. Ross does.

---

### Task 1: Do not record sensitive clipboard changes (Rust)

**Files:**
- Modify: `src/infrastructure/providers/src/clipboard_capture.rs` (add `PASSWORD_MANAGER_HINT` and
  `is_sensitive` after `classify`, and tests in its `mod tests`)
- Modify: `src/infrastructure/providers/src/clipboard_watcher.rs:21` (import) and `:188-192` (the
  `handle_change` early return)

**Acceptance Criteria:**
1. [ ] `pub fn is_sensitive(types: &[String]) -> bool` returns `true` exactly when one offered type equals
   `x-kde-passwordManagerHint`.
2. [ ] `handle_change` returns before `pull` when `is_sensitive(&types)` is true, logging
   `tracing::debug!("skipped sensitive clipboard change")` with no content.
3. [ ] Two new unit tests pass, and the existing `clipboard_capture` and `clipboard_watcher` tests still pass.
4. [ ] `cargo fmt --all -- --check` and `cargo clippy -p quantum-providers -- -D warnings` are clean.
5. [ ] No files changed outside the two listed.

**Step 0: Confirm the real signal (one-off check, protects the clipboard).**

Run:
```bash
SAVED="$(wl-paste --no-newline 2>/dev/null || true)"
printf 'plan-probe' | wl-copy --sensitive; sleep 0.3; wl-paste --list-types
printf '%s' "$SAVED" | wl-copy
```
Expected: the list includes `x-kde-passwordManagerHint`. If it does not, stop and report, because the design's
signal is wrong. Run `wl-copy` in the background (`&`) if it blocks the tool call, since it stays alive as the
clipboard owner.

**Step 1: Write the failing tests** in `clipboard_capture.rs` `mod tests`, which already has
`fn types(list: &[&str]) -> Vec<String>`:

```rust
    #[test]
    fn a_selection_with_the_password_manager_hint_is_sensitive() {
        assert!(is_sensitive(&types(&[
            "text/plain;charset=utf-8",
            "x-kde-passwordManagerHint",
        ])));
    }

    #[test]
    fn ordinary_text_is_not_sensitive() {
        assert!(!is_sensitive(&types(&[
            "text/plain;charset=utf-8",
            "UTF8_STRING",
        ])));
    }
```

**Step 2: Run them to see them fail.**
Run: `./scripts/devsh.sh cargo test -p quantum-providers clipboard_capture`
Expected: compile error, `cannot find function is_sensitive`.

**Step 3: Implement** after `classify` in `clipboard_capture.rs`:

```rust
/// The MIME type a password manager offers alongside a secret. `wl-copy
/// --sensitive` offers it too, and `wl-paste --watch` reports a selection
/// carrying it as `CLIPBOARD_STATE=sensitive`.
pub const PASSWORD_MANAGER_HINT: &str = "x-kde-passwordManagerHint";

/// Whether a clipboard selection is marked sensitive and must never be
/// recorded in the history.
pub fn is_sensitive(types: &[String]) -> bool {
    types.iter().any(|mime| mime == PASSWORD_MANAGER_HINT)
}
```

In `clipboard_watcher.rs`, change the import to
`use crate::clipboard_capture::{classify, is_sensitive, thumbnail, ClipKind};` and, in `handle_change`,
directly after the `let types = match …;` block:

```rust
        if is_sensitive(&types) {
            tracing::debug!("skipped sensitive clipboard change");
            return;
        }
```

**Step 4: Run the tests to see them pass.**
Run: `./scripts/devsh.sh cargo test -p quantum-providers clipboard`
Expected: all `clipboard*` tests pass.

**Step 5: Format and lint.**
Run: `./scripts/devsh.sh cargo fmt --all && ./scripts/devsh.sh cargo clippy -p quantum-providers -- -D warnings`
Expected: no diff after fmt; clippy clean.

**Step 6: Commit.**
```bash
git add src/infrastructure/providers/src/clipboard_capture.rs src/infrastructure/providers/src/clipboard_watcher.rs
git commit -m "feat: skip clipboard changes marked sensitive" -- src/infrastructure/providers/src/clipboard_capture.rs src/infrastructure/providers/src/clipboard_watcher.rs
```

---

### Task 2: Read the launcher's open arguments (pure helper)

**Files:**
- Create: `src/ui/plugins/launcher/views/launcher/src/lib/openArgs.ts`
- Create: `src/ui/plugins/launcher/views/launcher/src/lib/openArgs.test.ts`

**Acceptance Criteria:**
1. [ ] `initialQueryFrom(args: unknown): string` returns `args.query` when it is a string, otherwise `''`. That
   covers `null`, `undefined`, a non-object, a missing `query` and a non-string `query`.
2. [ ] `currentInitialQuery(): string` returns `initialQueryFrom(window.__quantum_args)`.
3. [ ] Six tests pass: a string, `null`, `undefined`, a number `query`, a missing `query`, and
   `currentInitialQuery` reading the window global.

**Step 1: Write the failing tests** in `openArgs.test.ts`:

```ts
import { afterEach, describe, expect, it } from 'vitest';
import { currentInitialQuery, initialQueryFrom } from './openArgs';

describe('initialQueryFrom', () => {
  it('returns a string query', () => {
    expect(initialQueryFrom({ query: ';' })).toBe(';');
  });
  it('returns empty for null', () => {
    expect(initialQueryFrom(null)).toBe('');
  });
  it('returns empty for undefined', () => {
    expect(initialQueryFrom(undefined)).toBe('');
  });
  it('returns empty for a non-string query', () => {
    expect(initialQueryFrom({ query: 7 })).toBe('');
  });
  it('returns empty when query is missing', () => {
    expect(initialQueryFrom({ path: '/tmp' })).toBe('');
  });
});

describe('currentInitialQuery', () => {
  afterEach(() => {
    delete (window as { __quantum_args?: unknown }).__quantum_args;
  });
  it('reads window.__quantum_args', () => {
    (window as { __quantum_args?: unknown }).__quantum_args = { query: ';' };
    expect(currentInitialQuery()).toBe(';');
  });
});
```

**Step 2: Run them to see them fail.**
Run: `./scripts/devsh.sh bash -c "cd src/ui/plugins/launcher/views/launcher && pnpm exec vitest run src/lib/openArgs.test.ts"`
Expected: FAIL, cannot resolve `./openArgs`.

**Step 3: Implement** `openArgs.ts`:

```ts
// The launcher can be opened with arguments, for example the Super+V bind's
// `quantumctl show launcher --args '{"query":";"}'`. The host injects them as
// `window.__quantum_args` on every open (`null` when an open carries none), so
// reading them each time the launcher is shown is always current. The files
// and file-viewer views read the same global.

export function initialQueryFrom(args: unknown): string {
  if (typeof args !== 'object' || args === null) {
    return '';
  }
  const query = (args as { query?: unknown }).query;
  if (typeof query === 'string') {
    return query;
  }
  return '';
}

export function currentInitialQuery(): string {
  return initialQueryFrom((window as { __quantum_args?: unknown }).__quantum_args);
}
```

**Step 4: Run them to see them pass.** Same command. Expected: 6 passed.

**Step 5: Commit.**
```bash
git add src/ui/plugins/launcher/views/launcher/src/lib/openArgs.ts src/ui/plugins/launcher/views/launcher/src/lib/openArgs.test.ts
git commit -m "feat: read the launcher's initial query from its open arguments" -- src/ui/plugins/launcher/views/launcher/src/lib/openArgs.ts src/ui/plugins/launcher/views/launcher/src/lib/openArgs.test.ts
```

---

### Task 3: Start the launcher from the initial query

**Files:**
- Modify: `src/ui/plugins/launcher/views/launcher/src/App.svelte`: import; `requeryOnShow` (`:308-312`); mount
  effect (`:328-333`)
- Modify: `src/ui/plugins/launcher/views/launcher/src/App.test.ts`: new `describe` block at the end

**Acceptance Criteria:**
1. [ ] Opened with `__quantum_args = { query: ';' }`, the launcher's input shows `;` and it calls
   `search` with `{ text: ';', providers: ['clipboard'] }`.
2. [ ] After that, a reopen (window `focus`) with `__quantum_args = null` resets the input to `''`.
3. [ ] With an app query typed, a reopen with `{ query: ';' }` switches the input to `;`.
4. [ ] All existing `App.test.ts` tests still pass, including "clears the search text when the launcher is
   reopened".

**Step 1: Write the failing tests** at the end of `App.test.ts`, inside the top-level `describe` or as a
sibling. Set the query with `fireEvent.input`, not `userEvent.type`, for the reason given in the comment at
`App.test.ts:386-388`.

```ts
describe('App.svelte open arguments', () => {
  beforeEach(() => {
    mockCall = vi.fn().mockResolvedValue({ matches: [] });
  });
  afterEach(() => {
    delete (window as { __quantum_args?: unknown }).__quantum_args;
  });

  it('opens in clipboard mode when shown with a ; query argument', async () => {
    (window as { __quantum_args?: unknown }).__quantum_args = { query: ';' };
    render(App);
    await act();
    const input = screen.getByPlaceholderText('Search...') as HTMLInputElement;
    await waitFor(() => expect(input.value).toBe(';'));
    await waitFor(() =>
      expect(mockCall).toHaveBeenCalledWith('search', { text: ';', providers: ['clipboard'] })
    );
  });

  it('returns to an empty query when reopened without arguments', async () => {
    (window as { __quantum_args?: unknown }).__quantum_args = { query: ';' };
    render(App);
    await act();
    const input = screen.getByPlaceholderText('Search...') as HTMLInputElement;
    await waitFor(() => expect(input.value).toBe(';'));
    (window as { __quantum_args?: unknown }).__quantum_args = null;
    window.dispatchEvent(new Event('focus'));
    await act();
    await waitFor(() => expect(input.value).toBe(''));
  });

  it('switches an app search to clipboard mode when reopened with a ; argument', async () => {
    render(App);
    await act();
    const input = screen.getByPlaceholderText('Search...') as HTMLInputElement;
    await fireEvent.input(input, { target: { value: 'fire' } });
    (window as { __quantum_args?: unknown }).__quantum_args = { query: ';' };
    window.dispatchEvent(new Event('focus'));
    await act();
    await waitFor(() => expect(input.value).toBe(';'));
  });
});
```
Add `afterEach` to the `vitest` import on line 1.

**Step 2: Run them to see them fail.**
Run: `./scripts/devsh.sh bash -c "cd src/ui/plugins/launcher/views/launcher && pnpm exec vitest run src/App.test.ts"`
Expected: the three new tests fail (input stays `''`).

**Step 3: Implement** in `App.svelte`:
- Add `import { currentInitialQuery } from './lib/openArgs';` beside the other `./lib` imports.
- In `requeryOnShow`, replace `handleSearch('');` with `handleSearch(currentInitialQuery());`, and add one
  line to the comment above it: the query comes from the open arguments (Super+V passes `;`), and is empty
  otherwise.
- In the mount effect, replace `untrack(() => handleSearch(''));` with
  `untrack(() => handleSearch(currentInitialQuery()));`.

**Step 4: Run the whole launcher suite.**
Run: `./scripts/devsh.sh bash -c "cd src/ui/plugins/launcher/views/launcher && pnpm exec vitest run"`
Expected: everything passes.

**Step 5: Commit.**
```bash
git commit -m "feat: open the launcher with the query from its arguments" -- src/ui/plugins/launcher/views/launcher/src/App.svelte src/ui/plugins/launcher/views/launcher/src/App.test.ts
```

---

### Task 4: Clipboard preview model (pure)

**Files:**
- Create: `src/ui/plugins/launcher/views/launcher/src/lib/clipboardPreview.ts`
- Create: `src/ui/plugins/launcher/views/launcher/src/lib/clipboardPreview.test.ts`

**Acceptance Criteria:**
1. [ ] `clipboardPreview(match: Match | undefined): ClipboardPreview | null` returns `null` for `undefined` or
   any provider other than `clipboard`.
2. [ ] A text entry (`action.kind === 'copy'` with string `data.text`) gives
   `{ kind: 'text', text, details }`, where `details` is `"<subtitle> · <size> · <n> line(s)"`.
3. [ ] An image entry (a `data_uri` icon) gives `{ kind: 'image', source: <data URI>, details: "<subtitle> · <title>" }`.
4. [ ] Anything else gives `{ kind: 'other', details: "<subtitle> · <title>" }`.
5. [ ] `humanSize`: `512` → `512 bytes`, `2048` → `2.0 KB`, `3145728` → `3.0 MB`. `lineCount`: `''` → 0,
   `'a'` → 1, `'a\nb'` → 2, `'a\nb\n'` → 2.
6. [ ] Size counts UTF-8 bytes (`'é'` is 2 bytes).

**Step 1: Write the failing tests** in `clipboardPreview.test.ts`. Use made-up values only.

```ts
import { describe, expect, it } from 'vitest';
import { clipboardPreview, humanSize, lineCount } from './clipboardPreview';
import type { Match } from './types';

function textMatch(text: string): Match {
  return {
    id: '1',
    provider: 'clipboard',
    title: text.split(/\s+/).join(' '),
    subtitle: '5m ago',
    score: 1,
    action: { kind: 'copy', data: { text } },
  };
}

describe('humanSize', () => {
  it('formats bytes, kilobytes and megabytes', () => {
    expect(humanSize(512)).toBe('512 bytes');
    expect(humanSize(2048)).toBe('2.0 KB');
    expect(humanSize(3145728)).toBe('3.0 MB');
  });
});

describe('lineCount', () => {
  it('counts lines and ignores one trailing newline', () => {
    expect(lineCount('')).toBe(0);
    expect(lineCount('a')).toBe(1);
    expect(lineCount('a\nb')).toBe(2);
    expect(lineCount('a\nb\n')).toBe(2);
  });
});

describe('clipboardPreview', () => {
  it('is null without a match or for another provider', () => {
    expect(clipboardPreview(undefined)).toBeNull();
    expect(clipboardPreview({ ...textMatch('x'), provider: 'desktop-apps' })).toBeNull();
  });

  it('previews the full copy text with age, size and line count', () => {
    expect(clipboardPreview(textMatch('first line\nsecond line'))).toEqual({
      kind: 'text',
      text: 'first line\nsecond line',
      details: '5m ago · 22 bytes · 2 lines',
    });
  });

  it('counts UTF-8 bytes and says line for one line', () => {
    expect(clipboardPreview(textMatch('é'))?.details).toBe('5m ago · 2 bytes · 1 line');
  });

  it('previews an image entry from its thumbnail', () => {
    const match: Match = {
      id: '2',
      provider: 'clipboard',
      title: 'Image 1280x720',
      subtitle: '1h ago',
      icon: { kind: 'data_uri', data: 'data:image/png;base64,AAAA' },
      score: 1,
      action: { kind: 'custom', data: { kind: 'clipboard', payload: { op: 'recopy', id: '2' } } },
    };
    expect(clipboardPreview(match)).toEqual({
      kind: 'image',
      source: 'data:image/png;base64,AAAA',
      details: '1h ago · Image 1280x720',
    });
  });

  it('falls back to age and title for other entries', () => {
    const match: Match = {
      id: '3',
      provider: 'clipboard',
      title: 'blob.bin',
      subtitle: '2d ago',
      score: 1,
      action: { kind: 'custom', data: { kind: 'clipboard', payload: { op: 'recopy', id: '3' } } },
    };
    expect(clipboardPreview(match)).toEqual({ kind: 'other', details: '2d ago · blob.bin' });
  });
});
```

**Step 2: Run them to see them fail.**
Run: `./scripts/devsh.sh bash -c "cd src/ui/plugins/launcher/views/launcher && pnpm exec vitest run src/lib/clipboardPreview.test.ts"`
Expected: FAIL, cannot resolve `./clipboardPreview`.

**Step 3: Implement** `clipboardPreview.ts`:

```ts
// What the launcher's clipboard-mode preview pane shows for the highlighted
// result. A text entry's primary action already carries its full text (the
// clipboard provider's `copy` action), so the preview shows exactly what Enter
// will copy. No extra IPC.
import type { Match } from './types';
import { isThumbnailIcon, resolveIcon } from './icon';

export type ClipboardPreview =
  | { kind: 'text'; text: string; details: string }
  | { kind: 'image'; source: string; details: string }
  | { kind: 'other'; details: string };

export function humanSize(bytes: number): string {
  if (bytes < 1024) {
    return `${bytes} bytes`;
  }
  if (bytes < 1024 * 1024) {
    return `${(bytes / 1024).toFixed(1)} KB`;
  }
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export function lineCount(text: string): number {
  if (text === '') {
    return 0;
  }
  return text.replace(/\n$/, '').split('\n').length;
}

function copyText(match: Match): string | undefined {
  if (match.action.kind !== 'copy') {
    return undefined;
  }
  const data = match.action.data as { text?: unknown } | null;
  if (data !== null && typeof data.text === 'string') {
    return data.text;
  }
  return undefined;
}

function joinDetails(parts: (string | undefined)[]): string {
  return parts.filter((part): part is string => Boolean(part)).join(' · ');
}

export function clipboardPreview(match: Match | undefined): ClipboardPreview | null {
  if (!match || match.provider !== 'clipboard') {
    return null;
  }
  const text = copyText(match);
  if (text !== undefined) {
    const lines = lineCount(text);
    const lineLabel = lines === 1 ? 'line' : 'lines';
    const size = humanSize(new TextEncoder().encode(text).length);
    return { kind: 'text', text, details: joinDetails([match.subtitle, size, `${lines} ${lineLabel}`]) };
  }
  const source = isThumbnailIcon(match.icon) ? resolveIcon(match.icon) : undefined;
  if (source) {
    return { kind: 'image', source, details: joinDetails([match.subtitle, match.title]) };
  }
  return { kind: 'other', details: joinDetails([match.subtitle, match.title]) };
}
```

**Step 4: Run them to see them pass.** Same command. Expected: all pass.

**Step 5: Commit.**
```bash
git add src/ui/plugins/launcher/views/launcher/src/lib/clipboardPreview.ts src/ui/plugins/launcher/views/launcher/src/lib/clipboardPreview.test.ts
git commit -m "feat: clipboard preview model for the launcher" -- src/ui/plugins/launcher/views/launcher/src/lib/clipboardPreview.ts src/ui/plugins/launcher/views/launcher/src/lib/clipboardPreview.test.ts
```

---

### Task 5: Side-by-side preview pane in clipboard mode

**Files:**
- Create: `src/ui/plugins/launcher/views/launcher/src/lib/ClipboardPreview.svelte`
- Modify: `src/ui/plugins/launcher/views/launcher/src/App.svelte` (derived state; markup `:337-375`)
- Modify: `src/ui/plugins/launcher/views/launcher/src/styles.css` (append)
- Modify: `src/ui/plugins/launcher/views/launcher/src/App.test.ts` (new tests)

**Acceptance Criteria:**
1. [ ] With a `;` query and clipboard matches, the card has class `clipboard-mode` and a
   `.clipboard-preview` shows the highlighted entry's full text inside `pre.clipboard-preview-text`, with
   `.clipboard-preview-details` above it.
2. [ ] ArrowDown changes the preview to the next entry's text.
3. [ ] With a plain query (`fire`), there is no `clipboard-mode` class and no `.clipboard-preview`.
4. [ ] While the `$` command-output panel shows, there is no preview, because `clipboardMode` requires
   `capture === null`.
5. [ ] CSS matches the playground decision: in clipboard mode the card is `900px` wide and `60vh` tall (720px
   on x1), keeping `max-width: 90vw`. The list takes 50%. The preview has a 1px `--color-border` left
   border. The details line is 12px `--color-muted`. The text is `--font-mono` at 12px with `pre-wrap` and
   `break-word`. The preview scrolls on its own and returns to the top when the entry changes. An image fills
   the pane width. Other modes keep the 600px card unchanged.
6. [ ] All launcher tests pass.

**Step 1: Write the failing tests** in `App.test.ts`, in a new `describe('App.svelte clipboard preview', …)`
with the same `beforeEach` and `afterEach` shape as Task 3:

```ts
  const clipboardMatches = [
    {
      id: 'a',
      provider: 'clipboard',
      title: 'first entry line one line two',
      subtitle: '1m ago',
      score: 1,
      action: { kind: 'copy', data: { text: 'first entry\nline one\nline two' } },
    },
    {
      id: 'b',
      provider: 'clipboard',
      title: 'second entry',
      subtitle: '9m ago',
      score: 1,
      action: { kind: 'copy', data: { text: 'second entry' } },
    },
  ];

  function mockClipboardSearch() {
    mockCall.mockImplementation((method: string, params: { providers?: string[] }) => {
      if (method === 'search' && params?.providers?.[0] === 'clipboard') {
        return Promise.resolve({ matches: clipboardMatches });
      }
      return Promise.resolve({ matches: [] });
    });
  }

  it('shows the highlighted entry in a preview pane in clipboard mode', async () => {
    mockClipboardSearch();
    const { container } = render(App);
    await act();
    const input = screen.getByPlaceholderText('Search...');
    await fireEvent.input(input, { target: { value: ';' } });
    await waitFor(() => expect(container.querySelector('.clipboard-preview-text')?.textContent).toBe('first entry\nline one\nline two'));
    expect(container.querySelector('.card')?.classList.contains('clipboard-mode')).toBe(true);
    expect(container.querySelector('.clipboard-preview-details')?.textContent).toBe('1m ago · 29 bytes · 3 lines');
  });

  it('moves the preview with the highlight', async () => {
    mockClipboardSearch();
    const { container } = render(App);
    await act();
    const input = screen.getByPlaceholderText('Search...');
    await fireEvent.input(input, { target: { value: ';' } });
    await waitFor(() => expect(container.querySelector('.clipboard-preview-text')).not.toBeNull());
    await fireEvent.keyDown(input, { key: 'ArrowDown' });
    await waitFor(() => expect(container.querySelector('.clipboard-preview-text')?.textContent).toBe('second entry'));
  });

  it('has no preview pane outside clipboard mode', async () => {
    mockClipboardSearch();
    const { container } = render(App);
    await act();
    const input = screen.getByPlaceholderText('Search...');
    await fireEvent.input(input, { target: { value: 'fire' } });
    await act();
    expect(container.querySelector('.clipboard-preview')).toBeNull();
    expect(container.querySelector('.card')?.classList.contains('clipboard-mode')).toBe(false);
  });
```

**Step 2: Run them to see them fail.**
Run: `./scripts/devsh.sh bash -c "cd src/ui/plugins/launcher/views/launcher && pnpm exec vitest run src/App.test.ts"`
Expected: the three new tests fail (no `.clipboard-preview-text`).

**Step 3: Implement.**

`lib/ClipboardPreview.svelte`:

```svelte
<script lang="ts">
  import type { ClipboardPreview } from './clipboardPreview';

  interface Props {
    preview: ClipboardPreview;
  }

  let { preview }: Props = $props();
  let body: HTMLDivElement | undefined = $state();

  // Start each newly highlighted entry at its top rather than wherever the
  // previous entry was scrolled to.
  $effect(() => {
    void preview;
    if (body) {
      body.scrollTop = 0;
    }
  });
</script>

<div class="clipboard-preview" aria-label="Clipboard entry preview">
  <div class="clipboard-preview-details">{preview.details}</div>
  <div class="clipboard-preview-body" bind:this={body}>
    {#if preview.kind === 'text'}
      <pre class="clipboard-preview-text">{preview.text}</pre>
    {:else if preview.kind === 'image'}
      <img class="clipboard-preview-image" src={preview.source} alt="" />
    {/if}
  </div>
</div>
```

`App.svelte`:
- Imports: `import ClipboardPreview from './lib/ClipboardPreview.svelte';` and
  `import { clipboardPreview } from './lib/clipboardPreview';`.
- After `activeDescendant`:

```ts
  // Clipboard mode (`;`) widens the card and shows the highlighted copy beside
  // the list. The $ output panel replaces the list, so no preview then.
  let clipboardMode = $derived(capture === null && providersForQuery(searchText)[0] === 'clipboard');
  let preview = $derived(clipboardMode ? clipboardPreview(matches[highlightedIndex]) : null);
```

- Markup: add `class:clipboard-mode={clipboardMode}` to the `.card` div. Wrap the existing
  `<div class="results-container">…</div>` without changing its contents:

```svelte
    <div class="results-body" class:split={clipboardMode}>
      <div class="results-container">
        <!-- existing {#if capture …}{:else if …} chain, unchanged -->
      </div>
      {#if clipboardMode}
        <div class="preview-container">
          {#if preview}
            <ClipboardPreview {preview} />
          {/if}
        </div>
      {/if}
    </div>
```

`styles.css`: append:

```css
/* Clipboard mode (`;`): the card widens and holds a fixed height so it never
   resizes as the highlight moves; the list takes the left half and the
   highlighted copy previews on the right. Settled in
   docs/playgrounds/2026-10-15-clipboard-preview-layout.html. */
.card.clipboard-mode {
  width: 900px;
  height: 60vh;
}

.results-body {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-height: 0;
}

.results-body.split {
  flex-direction: row;
}

.results-body.split .results-container {
  flex: none;
  width: 50%;
}

.preview-container {
  display: flex;
  flex: 1;
  min-width: 0;
  min-height: 0;
  border-left: 1px solid var(--color-border, #313244);
}

.clipboard-preview {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-width: 0;
  min-height: 0;
}

.clipboard-preview-details {
  padding: var(--space-2, 0.5rem) var(--space-3, 0.75rem);
  font-size: var(--font-size-sm, 12px);
  color: var(--color-muted, #6c7086);
  border-bottom: 1px solid var(--color-border, #313244);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.clipboard-preview-body {
  flex: 1;
  min-height: 0;
  overflow: auto;
  padding: var(--space-2, 0.5rem) var(--space-3, 0.75rem);
}

.clipboard-preview-text {
  margin: 0;
  white-space: pre-wrap;
  word-break: break-word;
  font-family: var(--font-mono, ui-monospace, 'SFMono-Regular', 'Menlo', 'Consolas', monospace);
  font-size: var(--font-size-sm, 12px);
  line-height: 1.5;
}

.clipboard-preview-image {
  display: block;
  width: 100%;
  height: auto;
  border-radius: var(--radius-md, 4px);
  border: 1px solid var(--color-border, #313244);
}
```

**Step 4: Run the whole launcher suite.**
Run: `./scripts/devsh.sh bash -c "cd src/ui/plugins/launcher/views/launcher && pnpm exec vitest run"`
Expected: everything passes.

**Step 5: Commit.**
```bash
git add src/ui/plugins/launcher/views/launcher/src/lib/ClipboardPreview.svelte
git commit -m "feat: side-by-side clipboard preview in the launcher" -- src/ui/plugins/launcher/views/launcher/src/lib/ClipboardPreview.svelte src/ui/plugins/launcher/views/launcher/src/App.svelte src/ui/plugins/launcher/views/launcher/src/App.test.ts src/ui/plugins/launcher/views/launcher/src/styles.css
```

---

### Task 6: Build, full checks, and a headless render against the playground

**Files:** none changed. Evidence goes to `/tmp/opencode/launcher-render/`.

**Acceptance Criteria:**
1. [ ] `just frontend-build` succeeds. `@quantum/client` builds first, so the build is topological.
2. [ ] `cargo build -p quantumd`, `cargo test --workspace`, `cargo fmt --all -- --check` and
   `cargo clippy --workspace -- -D warnings` all pass.
3. [ ] A headless screenshot of the built launcher in clipboard mode at 1920×1200 shows a 900px card, the
   list on the left half, and the preview on the right with the details line above wrapped monospace text.
   Compare it by eye with the playground's "Side by side" at 50%. Fix any difference before going on.
4. [ ] A second screenshot with `__quantum_args = null` shows today's 600px app launcher, unchanged.

**Step 1:** `./scripts/devsh.sh just frontend-build`, then `./scripts/devsh.sh cargo build -p quantumd`.

**Step 2:** `./scripts/devsh.sh cargo test --workspace`, then
`./scripts/devsh.sh cargo fmt --all -- --check`, then `./scripts/devsh.sh cargo clippy --workspace -- -D warnings`.

**Step 3: Render.**

Write `/tmp/opencode/launcher-render/render.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail
REPOSITORY_DIRECTORY="/home/ixiri/Documents/exocortex/code/projects/quantum"
WORK_DIRECTORY="/tmp/opencode/launcher-render"
DEFAULT_ARGUMENTS_JSON='{"query":";"}'
ARGUMENTS_JSON="${1:-$DEFAULT_ARGUMENTS_JSON}"
OUTPUT_NAME="${2:-clipboard}"
rm -rf "$WORK_DIRECTORY/site" && mkdir -p "$WORK_DIRECTORY/site"
cp -r "$REPOSITORY_DIRECTORY/src/ui/plugins/launcher/views/launcher/dist/." "$WORK_DIRECTORY/site/"
TOKENS=':root{--color-bg:#292520;--color-bg-alt:#3b3630;--color-border:#4a443e;--color-fg:#e8e4d8;--color-fg-alt:#d8d4c8;--color-accent:#a6e3a1;--color-muted:#8a8578;--color-error:#af4e3c;--color-overlay-backdrop:transparent;--color-shadow:rgba(10,8,6,0.6);--radius-lg:16px}html{background:#2d3324}'
cat > "$WORK_DIRECTORY/site/stub.js" <<STUB
window.__quantum_args = $ARGUMENTS_JSON;
const longText = ['Use the executing-plans skill to run the plan in code/projects/quantum.', '', 'Context:', '- Work on main; commit per task.', '- All builds go through ./scripts/devsh.sh.', '', 'Order:', '1. Rust: skip sensitive copies.', '2. Launcher: initial query from arguments.', '3. Launcher: the preview pane.', '', 'A deliberately long line to prove wrapping: ' + 'wrap '.repeat(60)].join('\n');
const clipboard = [
  { id: '1', provider: 'clipboard', title: longText.split(/\s+/).join(' ').slice(0, 80) + '…', subtitle: '2m ago', score: 1, action: { kind: 'copy', data: { text: longText } } },
  { id: '2', provider: 'clipboard', title: 'https://example.com/docs/clipboard', subtitle: '6m ago', score: 1, action: { kind: 'copy', data: { text: 'https://example.com/docs/clipboard' } } },
  { id: '3', provider: 'clipboard', title: 'just build && just check', subtitle: '1h ago', score: 1, action: { kind: 'copy', data: { text: 'just build && just check' } } },
];
const apps = [ { id: 'a', provider: 'desktop-apps', title: 'Firefox', subtitle: 'Web Browser', score: 1, action: { kind: 'launch', data: { desktop_id: 'firefox' } } } ];
window.webkit = { messageHandlers: { quantum: { postMessage(raw) {
  const request = JSON.parse(raw);
  let result = {};
  if (request.method === 'search') {
    const pinned = (request.params && request.params.providers) || [];
    result = { matches: pinned[0] === 'clipboard' ? clipboard : apps };
  }
  setTimeout(() => window.__quantum_resolve(request.id, result), 0);
} } } };
STUB
sed -i "s|/\* QUANTUM_TOKENS \*/|$TOKENS|" "$WORK_DIRECTORY/site/index.html"
sed -i 's#<script type="module"#<script src="./stub.js"></script><script type="module"#' "$WORK_DIRECTORY/site/index.html"
cd "$WORK_DIRECTORY/site"
python3 -m http.server 8765 >/dev/null 2>&1 &
SERVER_PID=$!
trap 'kill "$SERVER_PID"' EXIT
sleep 0.5
chromium --headless=new --window-size=1920,1200 --virtual-time-budget=5000 --screenshot="$WORK_DIRECTORY/$OUTPUT_NAME.png" "http://127.0.0.1:8765/index.html" 2>/dev/null
echo "$WORK_DIRECTORY/$OUTPUT_NAME.png"
```

Run `bash /tmp/opencode/launcher-render/render.sh` and `bash /tmp/opencode/launcher-render/render.sh null apps`.
Look at both images with the Read tool. If the stub's message shape does not match the bridge, read
`src/ui/packages/client/src/bridge.ts` and adjust the stub, not the view. Check `index.html` after
substituting: the `/* QUANTUM_TOKENS */` placeholder must be gone.

The harness was proven against the pre-change build on 2026-10-15: the stub answers `search`, the theme
renders, and that build ignores the arguments (it shows apps in both shots). After Tasks 3-5, the first shot
must change to clipboard mode.

**Step 4:** No commit. Record both screenshot paths for the report.

---

### Task 7: Document it in quantum and push

**Files:**
- Modify: `AGENTS.md`: the "Clipboard subsystem" bullet and the "Launcher power-up providers" bullet

**Acceptance Criteria:**
1. [ ] The clipboard bullet states: changes offering `x-kde-passwordManagerHint` (`wl-copy --sensitive`, KDE
   convention password managers) are never recorded (`is_sensitive`, `clipboard_capture.rs`).
2. [ ] It states: Super+V opens the launcher with `{"query":";"}` through `__quantum_args`; the launcher reads
   the arguments on every show (`lib/openArgs.ts`), and an argument-less open injects `null`, so Super+L stays
   empty.
3. [ ] It states: in clipboard mode the card is 900px by 60vh with a side-by-side preview built from the text
   result's `copy` action (`lib/clipboardPreview.ts`), so it shows exactly what Enter copies, and points to the
   design and the playground.
4. [ ] `git status -sb` shows `main` ahead of `origin/main` only by this plan's commits, plus the design and
   playground commits `a5640033` and `1c72da86`. If other sessions' commits are ahead, follow the root
   `AGENTS.md` rule: push only your own commits from a detached worktree on `origin/main`.
5. [ ] `git push origin main` succeeds, and `git rev-parse origin/main` equals the local `HEAD`.

**Steps:** edit, then
`git commit -m "docs: record sensitive-copy skipping, Super+V and the clipboard preview" -- AGENTS.md`, then
check ahead commits with `git log --oneline origin/main..HEAD`, then push.

---

### Task 8: Config repo — Super+V, remove cliphist and walker/elephant, build

Work in `code/infra/config` (the same checkout as `~/Documents/GitHub/config`). Start from a clean
`git status`. If it is not clean, stop and report: `just build` stages everything.

**Files:**
- Modify: `modules/features/home/hyprland.nix`: add the bind after
  `"$mod, m, exec, quantumctl show plugin/find-mouse/find-mouse"` (around `:218`); delete the three
  `exec-once` lines `:275-277`
- Modify: `modules/features/home/niri.nix:204-205`: delete both cliphist `spawn-at-startup` lines
- Modify: `modules/features/home/head.nix`: delete the `let … in` block (`elephantProviders`), the
  `walker.nix` import, the `xdg.configFile` elephant block, the `systemd.user.services.elephant` unit, and the
  `elephant` and `cliphist` packages. Keep `wl-clipboard`
- Delete: `modules/features/home/walker.nix`
- Modify: `modules/features/home/appimage-launcher.nix:4`: header comment "walker and quantum both read" →
  "quantum reads"
- Modify: `AGENTS.md:34`: drop `walker` from the home features list
- Modify: `flake.lock`: the `quantum` input only

**Acceptance Criteria:**
1. [ ] The Hyprland bind line is exactly
   `"$mod, v, exec, quantumctl show launcher --args '{\"query\":\";\"}'"`.
2. [ ] `rg -n -i 'cliphist|elephant|walker' modules/ AGENTS.md` matches nothing.
3. [ ] `nix flake update quantum` moves the lock to the pushed quantum `HEAD` from Task 7 (check
   `jq -r .nodes.quantum.locked.rev flake.lock`).
4. [ ] `just build` and `just check` pass.
5. [ ] Proof the build carries the change:
   `grep -a -c 'clipboard-preview-details' "$(nix-store -qR result | grep -E -- '-quantum-0\.1\.0$')/bin/.quantumd-wrapped"` > 0.
   The `bin/quantumd` wrappers are too small to grep, as the quantum `AGENTS.md` explains.
6. [ ] `nix-store -qR result | grep -E -c 'cliphist|elephant|walker'` is 0. If it is not 0, find the remaining
   referrer with `nix why-depends ./result <path>` and report it rather than forcing.
7. [ ] `nix-store -qR result | grep -E 'hyprland\.conf$' | xargs grep -n 'query'` shows the new bind.
8. [ ] One commit, made with explicit paths, pushed:
   `feat(x1): Super+V clipboard launcher; remove cliphist and walker/elephant`.

**Steps:** make the edits; `nix flake update quantum`; `just build`; `just check`; the three proofs; then
`git commit -m "…" -- <each path above, including the deleted walker.nix and flake.lock>`; `git push`.
After that, record the submodule pointers in the exocortex superrepo. The `post-commit` hook does it if the
superrepo is otherwise clean; if not, run `git add code/projects/quantum code/infra/config && git commit`.

Hand off to Ross: "run `just switch` in `code/infra/config`". Say it proves the bind, the new launcher, and the
removal on the live session, and that the agent cannot do it because activation needs your fingerprint or
sudo.

---

### Task 9: Agents copy secrets with `--sensitive`

**Files:**
- Modify: `code/agent-systems/opencode-profile/AGENTS.md:37` (the `wl-copy` bullet)
- Brain fact `45419bfa-6a54-40d5-a487-5bc8a0eef366` on `person:ross-chambers` ("Show multi-step manual setup as a
  document in his file viewer…")

**Acceptance Criteria:**
1. [ ] The bullet gains one sentence: "For a secret (a token, password or key), use `wl-copy --sensitive` so
   clipboard history does not record it."
2. [ ] The brain fact's "(put them on the clipboard with `wl-copy`)" becomes "(put them on the clipboard with
   `wl-copy --sensitive`, which clipboard history skips)". Do this with `brain_get` for the version, then
   `brain_update`.
3. [ ] opencode-profile committed with `docs: copy secrets with wl-copy --sensitive` and pushed. Check its
   visibility first (`gh repo view rosschambers/opencode-profile --json visibility`); the diff holds no
   secrets either way.

---

### Task 10: Live verification and cleanup (after Ross's `just switch`)

**Acceptance Criteria** (go through them with Ross one case at a time; for each, say what it proves):
1. [ ] `systemctl --user is-active quantum.service` is `active`, and the running binary is the new build:
   `grep -a -c clipboard-preview-details` on the service's `.quantumd-wrapped`, resolved from
   `/proc/<quantumd pid>/exe` or the unit's `ExecStart`.
2. [ ] Stop the stale recorders that Hyprland started at login. A switch does not end them. Name each one before
   killing it: `pgrep -af 'cliphist|elephant|walker'`, then `pkill -f 'wl-paste --type (text|image) --watch cliphist'`
   and, if still running, `systemctl --user stop elephant.service`. Afterwards `pgrep -af 'cliphist|elephant|walker'`
   is empty, and `pgrep -af 'wl-paste --watch'` lists only quantumd's watcher.
3. [ ] Before deleting, list each leftover store with its size and what it is (as in the design), then delete:
   `~/.cache/cliphist/`, `~/.cache/elephant/`, `~/.config/walker/`, `~/.config/elephant/`.
4. [ ] Ross: Super+V opens the launcher showing `;`, the clipboard list, and the preview of the newest copy.
5. [ ] Ross: arrow keys move the preview; a long entry scrolls inside the preview; the card does not resize.
6. [ ] Ross: Escape, then Super alone (Super+L), opens an empty app launcher at 600px. Releasing Super after
   Super+V does **not** toggle the launcher closed. If it does, the `bindr` on `$mod_L` fires after combos, and
   the bind needs another approach.
7. [ ] `printf 'plain-check-%s' "$(date +%s)" | wl-copy` appears at the top of `;`.
   `printf 'sensitive-check-%s' "$(date +%s)" | wl-copy --sensitive` does not, and
   `rg -c sensitive-check ~/.local/state/quantum/clipboard.json` finds nothing. Restore Ross's clipboard
   afterwards.
8. [ ] With Ross, delete the token-shaped entry still in quantum's history (`;`, highlight it, Ctrl+K, "Delete
   entry"). Show which entry it is without printing the value.
9. [ ] Log the session with `brain_log`, add a quantum project fact with `brain_add_fact` on `project:quantum`,
   and close nothing in the inbox that this did not finish.

If the arguments race with the focus event (Super+V sometimes opens empty), debug from evidence. Log
`currentInitialQuery()` on show under `QUANTUM_INSPECTOR=1`, and only then change the read timing, for
example to the next animation frame. Do not guess.
