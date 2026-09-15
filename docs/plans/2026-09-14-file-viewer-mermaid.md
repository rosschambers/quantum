# File Viewer Mermaid Rendering Implementation Plan

> **For OpenCode:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Render ```mermaid fenced code blocks as diagrams in the file-viewer plugin (`qv`), instead of falling through to highlight.js as plain text.

**Architecture:** In `MarkdownRenderer.svelte`, the `marked` code renderer gets a branch: when the fence language is `mermaid`, emit a placeholder `<div class="mermaid-block">` carrying the escaped diagram source as its text content, otherwise keep the existing highlight.js path. After the `{@html}` output mounts (or changes), an `$effect` dynamically imports mermaid (lazy — zero cost for markdown without diagrams), initializes it once with theme tokens read from CSS custom properties, and runs it over the placeholders (`mermaid.run({ nodes })` replaces each placeholder's text with SVG). A parse failure on one diagram falls back to a highlighted code block plus an error banner for that block only; other diagrams still render.

**Tech Stack:** Svelte 5 (runes), marked v14, mermaid v11 (npm dependency, bundled — no CDN), vitest + jsdom.

**Assumptions:**
- Mermaid is bundled locally via pnpm (`"mermaid": "^11"` in the file-viewer `package.json`). The webview serves from `dist/` through the `quantum://` scheme (`build.rs` stages the whole `dist/`; `scheme.rs` serves assets by extension), so dynamic-import chunks work and no network is needed. This bloats the view bundle by roughly 1 MB unbundled — accepted cost for the feature.
- mermaid `securityLevel: 'strict'` stays on (default): markdown files can be untrusted input, and strict mode escapes labels and sanitizes via DOMPurify inside the SVG output. Do not set `htmlLabels: true`.
- jsdom cannot measure SVG text (no layout engine), so `mermaid.run` is flaky under vitest for real diagrams. Unit tests therefore cover only the pure placeholder/escape logic; actual diagram rendering is verified manually with `qv` on a fixture file. Do not write a vitest test that asserts rendered SVG.
- Per repo AGENTS.md: setup work in Svelte components uses `$effect`, not `onMount` (vitest compatibility); pnpm runs from the workspace root `src/ui`; no abbreviations in code or comments.

**Target files (all under `src/ui/plugins/file-viewer/views/file-viewer/`):**
- Modify: `package.json` (add dependency)
- Create: `src/lib/mermaid.ts` (pure helpers + renderer bridge)
- Create: `src/lib/mermaid.test.ts`
- Modify: `src/lib/highlighter.ts` (export `escapeHtml`)
- Modify: `src/lib/MarkdownRenderer.svelte` (renderer branch + render effect)
- Modify: `src/lib/markdown.css` (`.mermaid-block` styles)

---

### Task 1: Add the mermaid dependency

**Files:**
- Modify: `src/ui/plugins/file-viewer/views/file-viewer/package.json`

**Acceptance Criteria:**
- [ ] `"mermaid": "^11"` appears in `dependencies` alongside `marked` and `highlight.js`
- [ ] `pnpm install` succeeds from the workspace root and writes the lockfile
- [ ] No other dependency changes

**Step 1: Add the dependency**

From the repo root:

```bash
./scripts/devsh.sh bash -c "cd src/ui && pnpm --filter file-viewer-panel add mermaid@^11"
```

(If pnpm is available on the host directly, plain `pnpm --filter file-viewer-panel add mermaid@^11` from `src/ui/` works too.)

**Step 2: Verify the view still builds**

```bash
./scripts/devsh.sh bash -c "cd src/ui && pnpm --filter file-viewer-panel build"
```

Expected: vite build succeeds, `dist/assets/` gains mermaid chunks. Allow up to 10 minutes (cold install).

**Step 3: Commit**

```bash
git add src/ui/plugins/file-viewer/views/file-viewer/package.json pnpm-lock.yaml
git commit -m "feat: add mermaid dependency to file-viewer"
```

---

### Task 2: Pure placeholder helpers (TDD)

**Files:**
- Modify: `src/lib/highlighter.ts` (export the existing `escapeHtml`)
- Create: `src/lib/mermaid.ts`
- Test: `src/lib/mermaid.test.ts`

**Acceptance Criteria:**
- [ ] `escapeHtml` is exported from `highlighter.ts` (behavior unchanged, still used internally)
- [ ] `isMermaidLanguage(lang: string | undefined): boolean` returns true only for exactly `'mermaid'` (case-insensitive)
- [ ] `mermaidPlaceholderHtml(source: string): string` returns `<div class="mermaid-block" data-mermaid-status="pending">${escapeHtml(source.trim())}</div>` — the source is HTML-escaped because it enters the document through `{@html}`
- [ ] Covered by tests listed below; no other files change

**Step 1: Write the failing tests**

Create `src/lib/mermaid.test.ts`:

```typescript
import { describe, it, expect } from 'vitest';
import { isMermaidLanguage, mermaidPlaceholderHtml } from './mermaid';

describe('isMermaidLanguage', () => {
    it('matches mermaid case-insensitively', () => {
        expect(isMermaidLanguage('mermaid')).toBe(true);
        expect(isMermaidLanguage('Mermaid')).toBe(true);
    });

    it('does not match other languages or undefined', () => {
        expect(isMermaidLanguage('javascript')).toBe(false);
        expect(isMermaidLanguage(undefined)).toBe(false);
        expect(isMermaidLanguage('mermaid-js')).toBe(false);
    });
});

describe('mermaidPlaceholderHtml', () => {
    it('wraps the source in a mermaid-block div', () => {
        expect(mermaidPlaceholderHtml('graph TD;\n  A --> B;')).toBe(
            '<div class="mermaid-block" data-mermaid-status="pending">graph TD;\n  A --&gt; B;</div>',
        );
    });

    it('escapes HTML-significant characters in the source', () => {
        expect(mermaidPlaceholderHtml('<script>alert("x")</script>')).toContain('&lt;script&gt;');
    });
});
```

**Step 2: Run tests to verify they fail**

```bash
cd src/ui/plugins/file-viewer/views/file-viewer && pnpm test
```

Expected: FAIL — cannot resolve `./mermaid`.

**Step 3: Write minimal implementation**

Export `escapeHtml` in `highlighter.ts` (change `function escapeHtml` to `export function escapeHtml`). Create `src/lib/mermaid.ts`:

```typescript
import { escapeHtml } from './highlighter';

export function isMermaidLanguage(language: string | undefined): boolean {
    return language?.toLowerCase() === 'mermaid';
}

export function mermaidPlaceholderHtml(source: string): string {
    return `<div class="mermaid-block" data-mermaid-status="pending">${escapeHtml(source.trim())}</div>`;
}
```

**Step 4: Run tests to verify they pass**

```bash
cd src/ui/plugins/file-viewer/views/file-viewer && pnpm test
```

Expected: PASS (3 tests).

**Step 5: Commit**

```bash
git add src/ui/plugins/file-viewer/views/file-viewer/src/lib/
git commit -m "feat: add mermaid placeholder helpers for the markdown renderer"
```

---

### Task 3: Wire the renderer branch and the render effect

**Files:**
- Modify: `src/lib/MarkdownRenderer.svelte`

**Acceptance Criteria:**
- [ ] `renderer.code` checks `isMermaidLanguage(token.lang)` first; mermaid fences emit `mermaidPlaceholderHtml(token.text)`, everything else keeps the existing highlight.js output unchanged
- [ ] An `$effect` (NOT `onMount`) runs after `{@html}` content mounts or changes, finds every `.mermaid-block[data-mermaid-status="pending"]`, and replaces it with rendered SVG
- [ ] Mermaid is lazy-loaded (`await import('mermaid')`) only when at least one placeholder exists — a markdown file with no diagrams never loads the mermaid bundle
- [ ] `mermaid.initialize` is called once before the first run with `startOnLoad: false`, `securityLevel: 'strict'`, `theme: 'base'`, and `themeVariables` mapped from CSS custom properties (`--color-fg`, `--color-bg`, `--color-accent`, `--font-sans`) with sensible hex fallbacks when unset
- [ ] A stale render (its placeholder was replaced by a re-render while awaiting) is discarded — check `element.isConnected` before injecting
- [ ] On per-diagram failure the placeholder falls back to `<div class="mermaid-block mermaid-error">…highlighted source…</div>` with the error message; other diagrams in the same document still render
- [ ] Existing non-mermaid rendering is unchanged (regression check: TOC anchors, code highlighting still work in the built view)

**Step 1: Add the branch in `renderer.code`**

At the top of `renderer.code` in `MarkdownRenderer.svelte`:

```typescript
if (isMermaidLanguage(language)) {
    return mermaidPlaceholderHtml(token.text);
}
```

**Step 2: Add the render effect**

In the `<script>` block (runes mode, no `onMount`):

```typescript
import { escapeHtml, highlightCode } from './highlighter';
import { isMermaidLanguage, mermaidPlaceholderHtml } from './mermaid';

let container: HTMLDivElement | undefined = $state();
let mermaidInitialized = $state(false);

$effect(() => {
    if (!parsedHtml || !container) return;
    const placeholders = Array.from(
        container.querySelectorAll<HTMLElement>('.mermaid-block[data-mermaid-status="pending"]'),
    );
    if (placeholders.length === 0) return;

    let cancelled = false;

    void (async () => {
        const { default: mermaid } = await import('mermaid');
        const styles = getComputedStyle(document.documentElement);
        const token = (name: string, fallback: string) =>
            styles.getPropertyValue(name).trim() || fallback;

        if (!mermaidInitialized) {
            mermaid.initialize({
                startOnLoad: false,
                securityLevel: 'strict',
                theme: 'base',
                themeVariables: {
                    fontFamily: token('--font-sans', 'system-ui'),
                    primaryColor: token('--color-surface', '#f0f2f5'),
                    primaryTextColor: token('--color-fg', '#1a1a1a'),
                    lineColor: token('--color-accent', '#5b6770'),
                },
            });
            mermaidInitialized = true;
        }

        for (const placeholder of placeholders) {
            if (cancelled || !placeholder.isConnected) continue;
            const source = placeholder.textContent ?? '';
            try {
                const { svg } = await mermaid.render(`mermaid-${crypto.randomUUID()}`, source);
                if (!placeholder.isConnected) continue;
                placeholder.innerHTML = svg;
                placeholder.removeAttribute('data-mermaid-status');
            } catch (error) {
                if (!placeholder.isConnected) continue;
                const message = error instanceof Error ? error.message : String(error);
                placeholder.classList.add('mermaid-error');
                placeholder.removeAttribute('data-mermaid-status');
                placeholder.innerHTML = `<p class="mermaid-error-message">Mermaid render failed: ${escapeHtml(message)}</p><pre><code class="hljs">${highlightCode(source, '')}</code></pre>`;
            }
        }
    })();

    return () => {
        cancelled = true;
    };
});
```

And bind the container div:

```svelte
<div class="markdown-renderer" bind:this={container}>
    {@html parsedHtml}
</div>
```

Note: `highlightCode(source, '')` falls into the auto-detect branch; pass `'mermaid'` instead if hljs throws on an unregistered language name (it should not — it catches and returns escaped code).

**Step 3: Manual verification build**

```bash
./scripts/devsh.sh bash -c "cd src/ui && pnpm --filter file-viewer-panel build"
```

Expected: build succeeds; mermaid lands in `dist/assets/` as lazy chunk(s) that are NOT part of the main entry chunk (check the build output listing — the main index chunk should not jump by ~1 MB).

**Step 4: Commit**

```bash
git add src/ui/plugins/file-viewer/views/file-viewer/src/lib/MarkdownRenderer.svelte
git commit -m "feat: render mermaid code fences as diagrams in the file viewer"
```

---

### Task 4: Styles for diagram blocks and error fallback

**Files:**
- Modify: `src/lib/markdown.css`

**Acceptance Criteria:**
- [ ] `.mermaid-block` centers its SVG, gives it breathing room, constrains overflow (`max-width: 100%` on the inner svg), and uses theme tokens with hex fallbacks (project rule — no hardcoded-color drift)
- [ ] `.mermaid-error` keeps a visible error message (`--color-error`) above the highlighted source
- [ ] Manual check in the built view: the sample graph below renders; a deliberately broken fence (`graph TD;\n  A -->` with a syntax error) shows the fallback banner and still renders other diagrams

**Step 1: Add styles to `markdown.css`**

```css
.markdown-renderer .mermaid-block {
    margin: 24px auto;
    padding: 16px;
    display: flex;
    justify-content: center;
    overflow-x: auto;
    background: var(--color-bg-alt, #f6f7f9);
    border: 1px solid var(--color-border, #ddd);
    border-radius: 8px;
    white-space: pre-wrap;
    font-family: var(--font-mono, monospace);
}

.markdown-renderer .mermaid-block svg {
    max-width: 100%;
    height: auto;
    white-space: normal;
    font-family: var(--font-sans, system-ui);
}

.markdown-renderer .mermaid-error {
    flex-direction: column;
    align-items: stretch;
    border-color: var(--color-error, #dc2626);
}

.markdown-renderer .mermaid-error-message {
    color: var(--color-error, #dc2626);
    font-family: var(--font-sans, system-ui);
    margin-bottom: 12px;
}
```

(The `.mermaid-block` base style doubles as the "pending" state styling while lazy rendering resolves — pending blocks briefly look like a quoted code panel, which is an honest placeholder.)

**Step 2: Verify in the live view**

Create `/tmp/mermaid-verify.md` containing the `digraph qualification { … }` graph from the feature request plus one broken fence (`graph TD;\n  A -->`). Rebuild (`pnpm --filter file-viewer-panel build`), run a dev daemon with `QUANTUM_PLUGIN_DIR=src/ui/plugins` per the repo README workflow (or restart the daemon if it already serves from disk), and `qv /tmp/mermaid-verify.md`.

Expected: the digraph renders as SVG in theme colors; the broken fence shows the red error banner with the original source below it; TOC and heading anchors still work.

**Step 3: Commit**

```bash
git add src/ui/plugins/file-viewer/views/file-viewer/src/lib/markdown.css
git commit -m "feat: style mermaid diagram blocks in the file viewer"
```

---

### Task 5: Full verification and documentation

**Files:**
- Verify only (no new files); optionally note the feature in the repo README/docs section for the file viewer if one exists

**Acceptance Criteria:**
- [ ] `pnpm --filter file-viewer-panel test` passes from `src/ui`
- [ ] `./scripts/devsh.sh cargo build -p quantumd` succeeds (dist embedding unchanged, but confirm no regression; allow 10 minutes)
- [ ] `./scripts/devsh.sh cargo test --workspace` passes
- [ ] Manual: open a markdown file WITHOUT mermaid fences — confirm (via devtools/`QUANTUM_INSPECTOR=1` network panel or by bundle inspection) that no mermaid chunk was fetched
- [ ] Manual: the sample digraph from the feature request renders in the live `qv` window with the sycamore theme active

**Step 1:** Run the test suites and build commands above, verify each acceptance line.

**Step 2: Commit any stragglers and update docs.** If the repo documents file-viewer capabilities (the AGENTS.md file-viewer paragraph mentions Markdown/JSON/code/images/video), append mermaid to that sentence:

```bash
git add AGENTS.md
git commit -m "docs: note mermaid rendering in the file viewer description"
```

---

## Manual test fixture

Save as `/tmp/mermaid-verify.md` for Task 4:

````markdown
# Mermaid Verification

```mermaid
digraph qualification {
  rankdir=LR;
  node [shape=box];
  authorize [label="Execution / workflow authorization"];
  foundation [label="01 foundation.ready"];
  bootstrap [label="02 runtime.bootstrap"];
  movement [label="02 moving-frame + two-client evidence"];
  source_art [label="03 concept + source region"];
  runtime_art [label="03 art.runtime-region candidate"];
  models [label="04 review.model-ready"];
  gates [label="04 review.gates-ready"];
  content [label="04 packaged content proof + reporting"];
  integrated [label="Actual asset + contact + both profiles"];
  human_art [label="Independent AI review → human art approval"];
  decision [label="Qualification verdict / next plan activation"];
  authorize -> foundation;
  foundation -> bootstrap;
  foundation -> source_art;
  foundation -> models;
  models -> gates;
  models -> source_art [label="descriptor validation"];
  gates -> source_art [label="concept review gate"];
  bootstrap -> movement;
  bootstrap -> runtime_art;
  source_art -> runtime_art;
  bootstrap -> content;
  gates -> content;
  runtime_art -> integrated;
  movement -> integrated;
  gates -> human_art;
  integrated -> human_art;
  human_art -> decision;
  content -> decision;
}
```

```mermaid
graph TD;
  A -->
```
````
