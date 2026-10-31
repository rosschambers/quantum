# qv markdown sanitizing and code-block copy button: implementation plan

> **For OpenCode:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Stop rendered markdown in qv from running JavaScript, then add a header strip with a copy button to every fenced code block.

**Architecture:**
- **Sanitizing:** all frontend, in the file viewer view.
  - Rendered markdown goes through DOMPurify before `{@html}`.
  - Graphviz SVG goes through DOMPurify's SVG profile before it reaches `innerHTML`.
  - The custom marked renderers escape what they splice in.
  - A `Content-Security-Policy` meta tag in the view's `index.html` blocks inline scripts as a second guard.
- **Copy button:** the marked code renderer emits a header strip (language name plus button). One delegated click handler calls `navigator.clipboard.writeText`. A live probe on 2026-10-31 showed qv is already a secure context, so the host needs no change.

**Tech Stack:** Svelte 5, marked 14, DOMPurify 3.4, highlight.js 11, vitest 2 with jsdom, WebKitGTK 2.54.1.

**Design:** `docs/plans/2026-10-31-qv-markdown-sanitize-and-code-copy-design.md`. Look reference: `docs/playgrounds/2026-10-31-markdown-code-copy-button.html`, preset "Header strip".

**Assumptions:**
- Work happens directly on `main` in `code/projects/quantum` (Ross's rule: no feature branches).
- Every command runs from the repository root through `./scripts/devsh.sh`.
- The view's package name is `file-viewer-panel`, and its directory is `src/ui/plugins/file-viewer/views/file-viewer` (written `VIEW` below).
- `dompurify@3.4.15` is already in the pnpm store as a mermaid dependency.
- marked 14 already escapes code-span text in its lexer, so the code-span renderer is left alone.
- marked 14 passes raw HTML through, and heading `token.text` is the raw source line.
- The host injects one inline `<script>` (the plugin-client bootstrap, `PLUGIN_CLIENT_SCRIPT` in `src/ui/host/src/scheme.rs`) into every plugin page.
  - The policy blocks it in qv. qv never uses it, because `@quantum/client` installs its own bridge globals (`src/ui/packages/client/src/bridge.ts:62`).
  - That one violation is expected and documented. Everything else must be violation-free.
- Host-injected JavaScript (`evaluate_javascript`) is not subject to the page's policy. Task 7 proves this live.

**Commit timing:** every `git commit` runs the pre-commit hook (workspace `cargo fmt --check` plus clippy). That takes about 35 seconds warm and up to 9 minutes cold. Give `git commit` a 30-minute tool timeout.

**Test command for this view** (used throughout):

```bash
./scripts/devsh.sh bash -c "cd src/ui/plugins/file-viewer/views/file-viewer && pnpm exec vitest run <file-or-pattern>"
```

---

### Task 1: Add DOMPurify as a direct dependency

**Files:**
- Modify: `VIEW/package.json`
- Modify: `src/ui/pnpm-lock.yaml`

**Acceptance Criteria:**
1. `VIEW/package.json` `dependencies` contains `"dompurify": "^3.4.15"`.
2. `pnpm-lock.yaml` records it as a direct dependency of `file-viewer-panel`.
3. The view's full test suite still passes.

**Step 1: Add the dependency**

```bash
./scripts/devsh.sh bash -c "cd src/ui && pnpm --filter file-viewer-panel add dompurify@^3.4.15"
```

Expected: pnpm reports `+ dompurify 3.4.15`.

**Step 2: Run the view's suite**

```bash
./scripts/devsh.sh bash -c "cd src/ui/plugins/file-viewer/views/file-viewer && pnpm test"
```

Expected: all tests pass.

**Step 3: Commit**

```bash
git add src/ui/plugins/file-viewer/views/file-viewer/package.json src/ui/pnpm-lock.yaml
git commit -m "chore: add dompurify as a direct file viewer dependency"
```

---

### Task 2: `sanitize.ts`, the pure sanitizers

**Files:**
- Create: `VIEW/src/lib/sanitize.ts`
- Test: `VIEW/src/lib/sanitize.test.ts`

**Acceptance Criteria:**
1. It exports `sanitizeMarkdownHtml(html: string): string` and `sanitizeSvg(svg: string): string`.
2. `sanitizeMarkdownHtml` removes each of these:
   - `<script>`, `<style>` and `<form>` (including their contents where DOMPurify drops them);
   - every `on*` attribute;
   - `javascript:` URLs;
   - `name` attributes.
3. `sanitizeMarkdownHtml` keeps:
   - `<details>`, `<summary>`, `<br>`, `<kbd>`, tables, spans and `<mark class data-*>`;
   - `<button type aria-label title class>` with an inline `<svg>` child, which the code header uses;
   - heading ids that shadow `document` properties, such as `images` and `title`;
   - `href` and `src` values that are `https:`, `http:`, `mailto:`, `file:`, relative, or `#fragment`;
   - `data:image/...` image sources.
4. `sanitizeSvg` keeps the `<svg>`, `<g>`, `<path>` and `<text>` structure. It removes `<script>`, `on*` attributes and `javascript:` links, including `xlink:href`.
5. All of the tests below pass.

**Step 1: Write the failing tests**

```ts
// VIEW/src/lib/sanitize.test.ts
import { describe, it, expect } from 'vitest';
import { sanitizeMarkdownHtml, sanitizeSvg } from './sanitize';

function parse(html: string): HTMLElement {
    const root = document.createElement('div');
    root.innerHTML = html;
    return root;
}

describe('sanitizeMarkdownHtml', () => {
    it('removes script, style and form elements', () => {
        const root = parse(sanitizeMarkdownHtml('<p>a</p><script>alert(1)</script><style>p{}</style><form><input></form>'));
        expect(root.querySelector('script, style, form')).toBeNull();
        expect(root.querySelector('p')?.textContent).toBe('a');
    });

    it('removes every event handler attribute', () => {
        const root = parse(sanitizeMarkdownHtml('<img src="x" onerror="alert(1)"><svg onload="alert(2)"></svg><details ontoggle="alert(3)" open><summary>s</summary></details>'));
        for (const element of Array.from(root.querySelectorAll('*'))) {
            for (const attribute of Array.from(element.attributes)) {
                expect(attribute.name.startsWith('on')).toBe(false);
            }
        }
    });

    it('removes javascript: links but keeps safe and relative ones', () => {
        const root = parse(sanitizeMarkdownHtml(
            '<a id="bad" href="javascript:alert(1)">x</a>' +
            '<a id="https" href="https://example.com">x</a>' +
            '<a id="mail" href="mailto:a@b.c">x</a>' +
            '<a id="relative" href="./other.md">x</a>' +
            '<a id="fragment" href="#task-2">x</a>',
        ));
        expect(root.querySelector('#bad')?.getAttribute('href')).toBeNull();
        expect(root.querySelector('#https')?.getAttribute('href')).toBe('https://example.com');
        expect(root.querySelector('#mail')?.getAttribute('href')).toBe('mailto:a@b.c');
        expect(root.querySelector('#relative')?.getAttribute('href')).toBe('./other.md');
        expect(root.querySelector('#fragment')?.getAttribute('href')).toBe('#task-2');
    });

    it('keeps file: and data: image sources (qv resolves relative images to file://)', () => {
        const root = parse(sanitizeMarkdownHtml(
            '<img id="file" src="file:///home/ixiri/a.png"><img id="data" src="data:image/png;base64,AAAA">',
        ));
        expect(root.querySelector('#file')?.getAttribute('src')).toBe('file:///home/ixiri/a.png');
        expect(root.querySelector('#data')?.getAttribute('src')).toBe('data:image/png;base64,AAAA');
    });

    it('keeps heading ids that shadow document properties, so table-of-contents anchors work', () => {
        const root = parse(sanitizeMarkdownHtml('<h2 id="images">Images</h2><h2 id="title">Title</h2>'));
        expect(root.querySelector('h2')?.id).toBe('images');
        expect(root.querySelectorAll('h2')[1]?.id).toBe('title');
    });

    it('removes name attributes so content cannot shadow document methods', () => {
        const root = parse(sanitizeMarkdownHtml('<img name="getElementById" src="a.png">'));
        expect(root.querySelector('img')?.hasAttribute('name')).toBe(false);
    });

    it('keeps the harmless raw HTML that Ross\'s documents use', () => {
        const html = '<details><summary>More</summary><p>a<br>b <kbd>Ctrl</kbd></p></details>' +
            '<table><tr><td><span>x</span></td></tr></table>' +
            '<mark class="search-match" data-original="true">m</mark>';
        const root = parse(sanitizeMarkdownHtml(html));
        expect(root.querySelector('details > summary')?.textContent).toBe('More');
        expect(root.querySelector('br')).not.toBeNull();
        expect(root.querySelector('kbd')?.textContent).toBe('Ctrl');
        expect(root.querySelector('td span')?.textContent).toBe('x');
        expect(root.querySelector('mark')?.getAttribute('data-original')).toBe('true');
    });

    it('keeps the code header button with its inline icon', () => {
        const html = '<button type="button" class="code-copy" aria-label="Copy code" title="Copy code">' +
            '<svg viewBox="0 0 24 24"><rect x="9" y="9" width="11" height="11"></rect></svg><span>Copy</span></button>';
        const button = parse(sanitizeMarkdownHtml(html)).querySelector('button.code-copy');
        expect(button?.getAttribute('type')).toBe('button');
        expect(button?.getAttribute('aria-label')).toBe('Copy code');
        expect(button?.querySelector('svg rect')).not.toBeNull();
    });
});

describe('sanitizeSvg', () => {
    it('keeps diagram structure and drops scripts, handlers and javascript: links', () => {
        const svg = '<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink">' +
            '<script>alert(1)</script>' +
            '<g id="node1" class="node" onclick="alert(2)"><path d="M0 0L1 1"></path><text>A</text></g>' +
            '<a xlink:href="javascript:alert(3)"><text>bad</text></a>' +
            '<a xlink:href="https://example.com"><text>good</text></a></svg>';
        const root = parse(sanitizeSvg(svg));
        expect(root.querySelector('svg')).not.toBeNull();
        expect(root.querySelector('g#node1 path')).not.toBeNull();
        expect(root.querySelector('script')).toBeNull();
        expect(root.querySelector('g#node1')?.hasAttribute('onclick')).toBe(false);
        expect(root.innerHTML).not.toContain('javascript:');
        expect(root.innerHTML).toContain('https://example.com');
    });
});
```

**Step 2: Run them and confirm they fail**

Run the test command with `src/lib/sanitize.test.ts`. Expected: FAIL, because `./sanitize` cannot be resolved.

**Step 3: Implement**

```ts
// VIEW/src/lib/sanitize.ts
// Sanitizers for HTML that qv builds from file content. Markdown may contain
// raw HTML, and marked passes it through untouched, so everything rendered with
// {@html} or innerHTML goes through here first. Design:
// docs/plans/2026-10-31-qv-markdown-sanitize-and-code-copy-design.md
import DOMPurify from 'dompurify';

// DOMPurify's default list without the exotic schemes, plus file: (qv resolves
// relative markdown images to file://). Relative references and #fragments
// still pass through the final two alternatives. javascript: and vbscript: never match.
const ALLOWED_URI_REGEXP = /^(?:(?:https?|mailto|file):|[^a-z]|[a-z+.-]+(?:[^a-z+.\-:]|$))/i;

export function sanitizeMarkdownHtml(html: string): string {
    return DOMPurify.sanitize(html, {
        FORBID_TAGS: ['style', 'form'],
        // `name` on images and forms shadows document properties; forbidding it
        // replaces DOMPurify's own clobbering check, which is switched off below.
        FORBID_ATTR: ['name'],
        ALLOWED_URI_REGEXP,
        // On, DOMPurify strips heading ids such as "images" or "title" because
        // they collide with document properties, which breaks table-of-contents anchors.
        SANITIZE_DOM: false,
    });
}

export function sanitizeSvg(svg: string): string {
    return DOMPurify.sanitize(svg, {
        USE_PROFILES: { svg: true, svgFilters: true },
        ALLOWED_URI_REGEXP,
    });
}
```

**Step 4: Run the tests and confirm they pass**

Run the test command with `src/lib/sanitize.test.ts`. Expected: PASS (9 tests).

If the `name` test fails, the cause is that DOMPurify applies `FORBID_ATTR` only to allowed attributes. Check the actual output rather than loosening the test.

**Step 5: Commit**

```bash
git add src/ui/plugins/file-viewer/views/file-viewer/src/lib/sanitize.ts src/ui/plugins/file-viewer/views/file-viewer/src/lib/sanitize.test.ts
git commit -m "feat: add markdown and SVG sanitizers to the file viewer"
```

---

### Task 3: Sanitize and escape in `MarkdownRenderer.svelte`

**Files:**
- Modify: `VIEW/src/lib/MarkdownRenderer.svelte`:
  - lines 1-17 (imports, and remove `mermaidRenderId`'s fallback);
  - lines 84-116 (the renderers and the parse result);
  - line 218 (the Graphviz `innerHTML`).
- Test: `VIEW/src/lib/MarkdownRenderer.test.ts` (add a `describe` block)

**Acceptance Criteria:**
1. `parsedHtml` is `sanitizeMarkdownHtml(marked.parse(content) as string)`.
2. The heading renderer:
   - renders its content with `this.parser.parseInline(token.tokens)`, so inline markdown renders and inline HTML is sanitized;
   - escapes the id with `escapeHtml`;
   - computes the id with `slugify(token.text)` exactly as before, so table-of-contents ids are unchanged.
3. The image renderer escapes `src`, `alt` and `title` with `escapeHtml`.
4. The code renderer escapes the language in its class attribute.
5. Graphviz SVG is passed through `sanitizeSvg` before `placeholder.innerHTML`.
6. `mermaidRenderId()` returns `` `mermaid-${crypto.randomUUID()}` ``. The counter, the `?.` fallback and the wrong secure-context comment are removed (the live probe showed `crypto.randomUUID` exists).
7. The new tests pass, and all existing `MarkdownRenderer.test.ts` tests still pass.

**Step 1: Write the failing tests** (append to `MarkdownRenderer.test.ts`)

```ts
describe('MarkdownRenderer sanitizing', () => {
    function attributeNames(root: Element): string[] {
        return Array.from(root.querySelectorAll('*')).flatMap((element) => Array.from(element.attributes).map((attribute) => attribute.name));
    }

    it('neutralizes raw HTML that would run script', () => {
        const content = [
            'hello <img src="does-not-exist" onerror="window.__pwned = true">',
            '',
            '<svg onload="window.__pwned = true"></svg>',
            '',
            '<script>window.__pwned = true</script>',
            '',
            '<a href="javascript:window.__pwned = true">link</a>',
            '',
            '<style>body { display: none }</style>',
            '',
            '<form action="https://example.com"><input name="x"></form>',
        ].join('\n');
        const { container } = render(MarkdownRenderer, { props: { content } });
        const root = container.querySelector('.markdown-renderer')!;
        expect(attributeNames(root).filter((name) => name.startsWith('on'))).toEqual([]);
        expect(root.querySelector('script, style, form')).toBeNull();
        expect(root.innerHTML).not.toContain('javascript:');
        expect((window as any).__pwned).toBeUndefined();
    });

    it('escapes an image title that tries to break out of its attribute', () => {
        const content = '![alt](a.png "x\\" onerror=\\"window.__pwned = true")';
        const { container } = render(MarkdownRenderer, { props: { content, fileDirectory: '/tmp' } });
        const image = container.querySelector('.markdown-renderer img')!;
        expect(image.hasAttribute('onerror')).toBe(false);
        expect(image.getAttribute('src')).toBe('file:///tmp/a.png');
    });

    it('renders inline markdown in headings, sanitizes inline HTML, and keeps the slug id', () => {
        const content = '# Images and **bold** <img src=x onerror="window.__pwned = true">';
        const { container } = render(MarkdownRenderer, { props: { content } });
        const heading = container.querySelector('h1.anchor-heading')!;
        expect(heading.querySelector('strong')?.textContent).toBe('bold');
        expect(heading.querySelector('img')?.hasAttribute('onerror')).toBe(false);
        // The id must stay exactly what the table of contents computes from the raw line.
        expect(heading.id).toBe(slugify('Images and **bold** <img src=x onerror="window.__pwned = true">'));
    });

    it('escapes a hostile fence language in the class attribute', () => {
        const content = '```js"onmouseover="window.__pwned=true\nconst a = 1;\n```';
        const { container } = render(MarkdownRenderer, { props: { content } });
        const code = container.querySelector('pre code')!;
        expect(code.hasAttribute('onmouseover')).toBe(false);
    });

    it('keeps the raw HTML Ross\'s documents use', () => {
        const content = '<details><summary>More</summary>\n\nInside <kbd>Ctrl</kbd><br/>next\n\n</details>';
        const { container } = render(MarkdownRenderer, { props: { content } });
        expect(container.querySelector('details > summary')?.textContent).toBe('More');
        expect(container.querySelector('kbd')?.textContent).toBe('Ctrl');
    });
});
```

Add `import { slugify } from './types';` to the test file's imports. Do not change `slugify`.

**Step 2: Run the tests and confirm they fail**

Run the test command with `src/lib/MarkdownRenderer.test.ts`. Expected: the new tests fail. In particular the first fails on the `onerror` attribute, and the heading test fails because `<strong>` is missing.

**Step 3: Implement**

Imports (top of the `<script>`):

```ts
import { marked, type Tokens } from 'marked';
import { escapeHtml, highlightCode } from './highlighter';
import { sanitizeMarkdownHtml, sanitizeSvg } from './sanitize';
```

Replace lines 10-17 (keep `let mermaidInitialized = false;`):

```ts
let mermaidInitialized = false;

function mermaidRenderId(): string {
    return `mermaid-${crypto.randomUUID()}`;
}
```

Replace the heading, image and code renderers and the return:

```ts
renderer.heading = function (this: typeof renderer, token: Tokens.Heading): string {
    const id = escapeHtml(slugify(token.text));
    const level = token.depth;
    const inner = this.parser.parseInline(token.tokens);
    return `<h${level} id="${id}" class="anchor-heading"><a class="anchor-link" href="#${id}">#</a>${inner}</h${level}>`;
};

renderer.code = (token) => {
    const language = token.lang || undefined;
    if (isMermaidLanguage(language)) {
        return diagramPlaceholderHtml(token.text, 'mermaid');
    }
    if (isGraphvizLanguage(language)) {
        return diagramPlaceholderHtml(token.text, 'graphviz');
    }
    const highlightedCode = highlightCode(token.text, language);
    const languageClass = language ? ` language-${escapeHtml(language)}` : '';
    return `<pre><code class="hljs${languageClass}">${highlightedCode}</code></pre>`;
};

renderer.image = (token) => {
    const src = escapeHtml(resolveImageSrc(token.href));
    const alt = escapeHtml(token.text || '');
    const titleAttribute = token.title ? ` title="${escapeHtml(token.title)}"` : '';
    return `<img src="${src}" alt="${alt}"${titleAttribute} />`;
};

marked.setOptions({ renderer });

return sanitizeMarkdownHtml(marked.parse(content) as string);
```

Also sanitize the parse-error fallback message (line 119), which interpolates an error message into HTML:

```ts
return `<p>Error rendering markdown: ${escapeHtml(error instanceof Error ? error.message : String(error))}</p>`;
```

Graphviz (line 212):

```ts
svg = sanitizeSvg(graphviz.layout(source, 'svg'));
```

**Step 4: Run the tests and confirm they pass**

```bash
./scripts/devsh.sh bash -c "cd src/ui/plugins/file-viewer/views/file-viewer && pnpm test"
```

Expected: the whole view suite passes, including the existing search tests. The authored `<mark data-original>` test proves DOMPurify keeps `class` and `data-*`.

**Step 5: Commit**

```bash
git add src/ui/plugins/file-viewer/views/file-viewer/src/lib/MarkdownRenderer.svelte src/ui/plugins/file-viewer/views/file-viewer/src/lib/MarkdownRenderer.test.ts
git commit -m "fix: sanitize rendered markdown and Graphviz output in the file viewer"
```

---

### Task 4: Content Security Policy for the file viewer page

**Files:**
- Modify: `VIEW/index.html`
- Test: `VIEW/src/lib/contentSecurityPolicy.test.ts` (new)

**Acceptance Criteria:**
1. `index.html` has exactly one `<meta http-equiv="Content-Security-Policy">`, placed before every `<script>` and `<style>` element.
2. The policy's `script-src` contains neither `'unsafe-inline'` nor `'unsafe-eval'`.
3. The policy's `object-src`, `base-uri`, `form-action` and `frame-src` are all `'none'`.
4. A comment next to the tag names the one expected violation: the host's plugin-client bootstrap, which qv does not use.
5. The test below passes.

**Step 1: Write the failing test**

```ts
// VIEW/src/lib/contentSecurityPolicy.test.ts
import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

const html = readFileSync(resolve(__dirname, '../../index.html'), 'utf8');

function directive(policy: string, name: string): string[] {
    const entry = policy.split(';').map((part) => part.trim()).find((part) => part.startsWith(`${name} `));
    return entry ? entry.split(/\s+/).slice(1) : [];
}

describe('file viewer Content Security Policy', () => {
    const document = new DOMParser().parseFromString(html, 'text/html');
    const metas = document.querySelectorAll('meta[http-equiv="Content-Security-Policy"]');
    const policy = metas[0]?.getAttribute('content') ?? '';

    it('declares exactly one policy, before any script or style', () => {
        expect(metas).toHaveLength(1);
        const headChildren = Array.from(document.head.children);
        const policyIndex = headChildren.indexOf(metas[0]);
        const firstActive = headChildren.findIndex((element) => element.tagName === 'SCRIPT' || element.tagName === 'STYLE');
        expect(policyIndex).toBeLessThan(firstActive);
    });

    it('never allows inline or evaluated script', () => {
        const scripts = directive(policy, 'script-src');
        expect(scripts.length).toBeGreaterThan(0);
        expect(scripts).not.toContain("'unsafe-inline'");
        expect(scripts).not.toContain("'unsafe-eval'");
    });

    it('shuts plugins, base rewriting, form posts and frames', () => {
        for (const name of ['object-src', 'base-uri', 'form-action', 'frame-src']) {
            expect(directive(policy, name)).toEqual(["'none'"]);
        }
    });
});
```

**Step 2: Run it and confirm it fails**

Run the test command with `src/lib/contentSecurityPolicy.test.ts`. Expected: FAIL, `expected [] to have a length of 1`.

**Step 3: Implement** (`VIEW/index.html`)

```html
<!doctype html>
<html>
  <head>
    <meta charset="utf-8" />
    <!--
      Second guard behind sanitize.ts: no inline or evaluated script can run in
      the file viewer. 'wasm-unsafe-eval' is for the Graphviz WebAssembly module.
      Inline styles stay allowed for the theme tokens below, Svelte and mermaid.
      quantum: covers host-served images on other quantum:// hosts.
      Expected violation: the host injects an inline plugin-client bootstrap
      (PLUGIN_CLIENT_SCRIPT in src/ui/host/src/scheme.rs) into every plugin page.
      qv never uses it (@quantum/client installs its own bridge globals), so the
      policy blocking it is intended.
    -->
    <meta
      http-equiv="Content-Security-Policy"
      content="default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline'; img-src 'self' quantum: file: data: blob: https: http:; media-src 'self' quantum: file: blob:; font-src 'self' data:; connect-src 'self'; object-src 'none'; base-uri 'none'; form-action 'none'; frame-src 'none'"
    />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <style id="quantum-tokens">/* QUANTUM_TOKENS */</style>
    <script type="module" src="src/main.ts"></script>
  </head>
  <body><div id="app"></div></body>
</html>
```

**Step 4: Run it and confirm it passes**

Run the test command with `src/lib/contentSecurityPolicy.test.ts`. Expected: PASS (3 tests). Then run `pnpm build` in `VIEW` (through `devsh.sh`) and confirm `dist/index.html` still contains the meta tag before the generated `<script type="module">`.

**Step 5: Commit**

```bash
git add src/ui/plugins/file-viewer/views/file-viewer/index.html src/ui/plugins/file-viewer/views/file-viewer/src/lib/contentSecurityPolicy.test.ts
git commit -m "feat: add a Content Security Policy to the file viewer page"
```

The exact source list is only provisional until Task 7 runs it live in WebKit.

---

### Task 5: Code header strip and copy button

**Files:**
- Create: `VIEW/src/lib/codeCopy.ts`
- Test: `VIEW/src/lib/codeCopy.test.ts`
- Modify: `VIEW/src/lib/MarkdownRenderer.svelte`:
  - the code renderer from Task 3;
  - a new `$effect` that attaches the click handler.
- Modify: `VIEW/src/lib/markdown.css` (append the header and button styles)
- Test: `VIEW/src/lib/MarkdownRenderer.test.ts` (add a `describe` block)

**Acceptance Criteria:**
1. `codeCopy.ts` exports:
   - `codeHeaderHtml(language: string | undefined): string`;
   - `handleCodeCopyClick(event: Event): void`;
   - `COPY_FEEDBACK_MILLISECONDS = 1500`.
2. Each fenced, non-diagram block renders as `div.code-block > div.code-header + pre > code.hljs`.
3. The header contains:
   - `span.code-language`, with the escaped fence language, or empty;
   - `button.code-copy` with `type="button"`, `aria-label="Copy code"`, `title="Copy code"`, a copy icon and the text "Copy".
4. A click calls `navigator.clipboard.writeText` with exactly that block's `code.textContent`. Entities are decoded, and leading indentation is kept.
5. On success:
   - the button's `data-copy-state` is `copied`;
   - the label reads "Copied" with a check icon;
   - after `COPY_FEEDBACK_MILLISECONDS` it returns to `idle` with "Copy".
6. On failure (rejection, or `navigator.clipboard` undefined), `data-copy-state` is `failed` and the label reads "Copy failed". It also resets after the same time.
7. A second click during feedback restarts the timer rather than stacking timers.
8. While feedback shows, the button's `min-width` is at least its idle width.
9. Mermaid and Graphviz fences get no header.
10. All the tests below pass.

**Step 1: Write the failing tests**

```ts
// VIEW/src/lib/codeCopy.test.ts
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest';
import { codeHeaderHtml, handleCodeCopyClick, COPY_FEEDBACK_MILLISECONDS } from './codeCopy';

function mountBlock(code: string, language?: string): HTMLButtonElement {
    document.body.innerHTML = `<div class="code-block">${codeHeaderHtml(language)}<pre><code class="hljs"></code></pre></div>`;
    document.querySelector('code')!.textContent = code;
    document.body.addEventListener('click', handleCodeCopyClick);
    return document.querySelector('button.code-copy')!;
}

function stubClipboard(writeText: (text: string) => Promise<void>): void {
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
}

beforeEach(() => vi.useFakeTimers());
afterEach(() => {
    document.body.removeEventListener('click', handleCodeCopyClick);
    document.body.innerHTML = '';
    vi.useRealTimers();
    Object.defineProperty(navigator, 'clipboard', { value: undefined, configurable: true });
});

describe('codeHeaderHtml', () => {
    it('renders the language and an accessible copy button', () => {
        const button = mountBlock('x', 'rust');
        expect(document.querySelector('.code-language')?.textContent).toBe('rust');
        expect(button.getAttribute('type')).toBe('button');
        expect(button.getAttribute('aria-label')).toBe('Copy code');
        expect(button.getAttribute('title')).toBe('Copy code');
        expect(button.textContent).toBe('Copy');
        expect(button.dataset.copyState).toBe('idle');
    });

    it('escapes a hostile language name', () => {
        mountBlock('x', '<img src=x onerror=alert(1)>');
        expect(document.querySelector('.code-language img')).toBeNull();
        expect(document.querySelector('.code-language')?.textContent).toBe('<img src=x onerror=alert(1)>');
    });

    it('leaves the language empty when the fence names none', () => {
        mountBlock('x');
        expect(document.querySelector('.code-language')?.textContent).toBe('');
    });
});

describe('handleCodeCopyClick', () => {
    it('copies exactly the block text and shows Copied, then resets', async () => {
        const writeText = vi.fn(async () => {});
        stubClipboard(writeText);
        const button = mountBlock('  const s = "<b>&amp;";\n  return s;');
        button.click();
        await vi.waitFor(() => expect(button.dataset.copyState).toBe('copied'));
        expect(writeText).toHaveBeenCalledWith('  const s = "<b>&amp;";\n  return s;');
        expect(button.textContent).toBe('Copied');
        vi.advanceTimersByTime(COPY_FEEDBACK_MILLISECONDS);
        expect(button.dataset.copyState).toBe('idle');
        expect(button.textContent).toBe('Copy');
    });

    it('shows Copy failed when the write rejects', async () => {
        stubClipboard(async () => { throw new Error('denied'); });
        const button = mountBlock('x');
        button.click();
        await vi.waitFor(() => expect(button.dataset.copyState).toBe('failed'));
        expect(button.textContent).toBe('Copy failed');
        vi.advanceTimersByTime(COPY_FEEDBACK_MILLISECONDS);
        expect(button.dataset.copyState).toBe('idle');
    });

    it('shows Copy failed when the clipboard API is missing', async () => {
        const button = mountBlock('x');
        button.click();
        await vi.waitFor(() => expect(button.dataset.copyState).toBe('failed'));
    });

    it('restarts the feedback timer on a second click instead of stacking', async () => {
        stubClipboard(async () => {});
        const button = mountBlock('x');
        button.click();
        await vi.waitFor(() => expect(button.dataset.copyState).toBe('copied'));
        vi.advanceTimersByTime(COPY_FEEDBACK_MILLISECONDS - 100);
        button.click();
        await vi.waitFor(() => expect(button.dataset.copyState).toBe('copied'));
        vi.advanceTimersByTime(200);
        expect(button.dataset.copyState).toBe('copied');
        vi.advanceTimersByTime(COPY_FEEDBACK_MILLISECONDS);
        expect(button.dataset.copyState).toBe('idle');
    });

    it('ignores clicks that are not on a copy button', () => {
        const writeText = vi.fn(async () => {});
        stubClipboard(writeText);
        mountBlock('x');
        document.querySelector<HTMLElement>('code')!.click();
        expect(writeText).not.toHaveBeenCalled();
    });
});
```

Append to `MarkdownRenderer.test.ts`:

```ts
describe('MarkdownRenderer code header', () => {
    it('wraps fenced code in a header strip with a copy button, but never diagrams', async () => {
        const content = '```bash\necho hi\n```\n\n```\nplain\n```\n\n```mermaid\ngraph TD;\n  A --> B;\n```';
        const { container } = render(MarkdownRenderer, { props: { content } });
        const blocks = container.querySelectorAll('.code-block');
        expect(blocks).toHaveLength(2);
        expect(blocks[0].querySelector('.code-language')?.textContent).toBe('bash');
        expect(blocks[1].querySelector('.code-language')?.textContent).toBe('');
        expect(container.querySelector('.diagram-block .code-header')).toBeNull();
    });

    it('copies the block it belongs to when its button is clicked', async () => {
        const writeText = vi.fn(async () => {});
        Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
        try {
            const content = '```ts\n  const a = 1;\n```\n\n```ts\nconst b = 2;\n```';
            const { container } = render(MarkdownRenderer, { props: { content } });
            container.querySelectorAll<HTMLButtonElement>('button.code-copy')[1].click();
            await vi.waitFor(() => expect(writeText).toHaveBeenCalledWith('const b = 2;'));
            container.querySelectorAll<HTMLButtonElement>('button.code-copy')[0].click();
            await vi.waitFor(() => expect(writeText).toHaveBeenLastCalledWith('  const a = 1;'));
        } finally {
            Object.defineProperty(navigator, 'clipboard', { value: undefined, configurable: true });
        }
    });
});
```

**Step 2: Run them and confirm they fail**

Run the test command with `src/lib/codeCopy.test.ts src/lib/MarkdownRenderer.test.ts`. Expected: FAIL, because `./codeCopy` cannot be resolved and the header tests find no `.code-block`.

**Step 3: Implement `codeCopy.ts`**

```ts
// VIEW/src/lib/codeCopy.ts
// Header strip and copy button for fenced code blocks in rendered markdown.
// Look settled in docs/playgrounds/2026-10-31-markdown-code-copy-button.html
// ("Header strip"). The page is already a secure context (live probe,
// 2026-10-31), so navigator.clipboard.writeText works from a click.
import { escapeHtml } from './highlighter';

export const COPY_FEEDBACK_MILLISECONDS = 1500;

type CopyState = 'idle' | 'copied' | 'failed';

const ICON_COPY = '<svg class="code-copy-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="9" y="9" width="11" height="11" rx="2"></rect><path d="M5 15V6a2 2 0 0 1 2-2h9"></path></svg>';
const ICON_CHECK = '<svg class="code-copy-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 12.5l4.5 4.5L19 7.5"></path></svg>';
const ICON_ALERT = '<svg class="code-copy-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" aria-hidden="true"><path d="M12 7v6"></path><path d="M12 17h.01"></path><circle cx="12" cy="12" r="9"></circle></svg>';

const CONTENT: Record<CopyState, string> = {
    idle: `${ICON_COPY}<span class="code-copy-label">Copy</span>`,
    copied: `${ICON_CHECK}<span class="code-copy-label">Copied</span>`,
    failed: `${ICON_ALERT}<span class="code-copy-label">Copy failed</span>`,
};

const resetTimers = new WeakMap<HTMLButtonElement, ReturnType<typeof setTimeout>>();

export function codeHeaderHtml(language: string | undefined): string {
    return `<div class="code-header"><span class="code-language">${escapeHtml(language ?? '')}</span>` +
        `<button type="button" class="code-copy" data-copy-state="idle" aria-label="Copy code" title="Copy code">${CONTENT.idle}</button></div>`;
}

function showState(button: HTMLButtonElement, state: CopyState): void {
    button.dataset.copyState = state;
    button.innerHTML = CONTENT[state];
}

async function writeToClipboard(text: string): Promise<boolean> {
    if (typeof navigator.clipboard?.writeText !== 'function') return false;
    try {
        await navigator.clipboard.writeText(text);
        return true;
    } catch (error) {
        console.error('Copying code failed:', error);
        return false;
    }
}

export function handleCodeCopyClick(event: Event): void {
    const target = event.target;
    if (!(target instanceof Element)) return;
    const button = target.closest<HTMLButtonElement>('button.code-copy');
    if (!button) return;
    const code = button.closest('.code-block')?.querySelector('pre code');
    if (!code) return;

    const idleWidth = button.getBoundingClientRect().width;
    void writeToClipboard(code.textContent ?? '').then((copied) => {
        clearTimeout(resetTimers.get(button));
        showState(button, copied ? 'copied' : 'failed');
        // Never shrink while feedback shows; grow only if the longer label needs it.
        button.style.minWidth = `${Math.max(idleWidth, button.getBoundingClientRect().width)}px`;
        resetTimers.set(button, setTimeout(() => {
            showState(button, 'idle');
            button.style.minWidth = '';
        }, COPY_FEEDBACK_MILLISECONDS));
    });
}
```

Note: `textContent` of an SVG plus a span is just the label text, which is why the tests expect `button.textContent` to be `Copy` or `Copied`.

**Step 4: Wire it into `MarkdownRenderer.svelte`**

Import: `import { codeHeaderHtml, handleCodeCopyClick } from './codeCopy';`

Code renderer: the final return becomes:

```ts
return `<div class="code-block">${codeHeaderHtml(language)}<pre><code class="hljs${languageClass}">${highlightedCode}</code></pre></div>`;
```

New effect, placed after the diagram effect:

```ts
// One delegated listener for every code block's copy button. Attached here,
// not as an onclick attribute, so sanitized content can never add handlers.
$effect(() => {
    if (!container) return;
    const element = container;
    element.addEventListener('click', handleCodeCopyClick);
    return () => element.removeEventListener('click', handleCodeCopyClick);
});
```

**Step 5: Styles** (append to `markdown.css`; values from the playground's "Header strip" preset)

```css
/* Fenced code: header strip (language name and copy button) above the block.
   Settled in docs/playgrounds/2026-10-31-markdown-code-copy-button.html. */
.markdown-renderer .code-block {
    margin-bottom: 0.8em;
}

.markdown-renderer .code-block pre {
    margin-bottom: 0;
    border-top: none;
    border-radius: 0 0 var(--radius-md, 6px) var(--radius-md, 6px);
}

.markdown-renderer .code-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    background: var(--color-bg-alt, #f5f5f5);
    border: 1px solid var(--color-border, #ddd);
    border-bottom: 1px solid var(--color-divider, #ddd);
    border-radius: var(--radius-md, 6px) var(--radius-md, 6px) 0 0;
    padding: 4px 6px 4px 14px;
    font-family: var(--font-mono, 'monospace');
    font-size: 12px;
    color: var(--color-muted, #999);
    user-select: none;
    -webkit-user-select: none;
}

.markdown-renderer .code-copy {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    height: 24px;
    padding: 0 8px;
    border: 1px solid transparent;
    border-radius: 5px;
    background: transparent;
    color: var(--color-fg-alt, #666);
    font-family: var(--font-sans, system-ui);
    font-size: 12px;
    line-height: 1;
    white-space: nowrap;
    cursor: pointer;
    transition: background-color 120ms, color 120ms;
}

.markdown-renderer .code-copy:hover {
    background: var(--color-surface-hover, #eee);
}

.markdown-renderer .code-copy:focus-visible {
    outline: 2px solid var(--color-accent, #007bff);
    outline-offset: 1px;
}

.markdown-renderer .code-copy[data-copy-state='copied'] {
    color: var(--color-accent, #007bff);
}

.markdown-renderer .code-copy[data-copy-state='failed'] {
    color: var(--color-error, #dc2626);
}

.markdown-renderer .code-copy-icon {
    width: 15px;
    height: 15px;
    flex: none;
}
```

**Step 6: Run the tests and confirm they pass**

```bash
./scripts/devsh.sh bash -c "cd src/ui/plugins/file-viewer/views/file-viewer && pnpm test"
```

Expected: the whole view suite passes.

**Step 7: Commit**

```bash
git add src/ui/plugins/file-viewer/views/file-viewer/src/lib/codeCopy.ts src/ui/plugins/file-viewer/views/file-viewer/src/lib/codeCopy.test.ts src/ui/plugins/file-viewer/views/file-viewer/src/lib/MarkdownRenderer.svelte src/ui/plugins/file-viewer/views/file-viewer/src/lib/MarkdownRenderer.test.ts src/ui/plugins/file-viewer/views/file-viewer/src/lib/markdown.css
git commit -m "feat: add a header strip with a copy button to markdown code blocks in qv"
```

---

### Task 6: Keep the code header out of search

**Files:**
- Modify: `VIEW/src/lib/search.ts:105-107`, `:130` and the doc comment at `:166-174`
- Test: `VIEW/src/lib/MarkdownRenderer.test.ts` (add to the code header `describe`)

**Acceptance Criteria:**
1. `isDiagramElement` is renamed `isExcludedFromSearch` everywhere (a full rename, not an alias). It returns true for `diagram-block`, `diagram-error` and `code-header`.
2. The doc comment on `findMatchesInDom` lists all three classes.
3. Searching a document whose only code block is ```` ```bash ```` containing `echo needle` gives:
   - `copy`: 0 matches;
   - `bash`: 0 matches;
   - `needle`: 1 match.
4. The whole view suite passes.

**Step 1: Write the failing test**

```ts
it('never counts the language name or the button label as search matches', async () => {
    const counts: number[] = [];
    const onMatchCount = (count: number) => counts.push(count);
    const content = '```bash\necho needle\n```';
    const { rerender } = render(MarkdownRenderer, { props: { content, query: 'copy', onMatchCount } });
    await vi.waitFor(() => expect(counts.at(-1)).toBe(0));
    await rerender({ content, query: 'bash', onMatchCount });
    await vi.waitFor(() => expect(counts.at(-1)).toBe(0));
    await rerender({ content, query: 'needle', onMatchCount });
    await vi.waitFor(() => expect(counts.at(-1)).toBe(1));
});
```

**Step 2: Run it and confirm it fails**

Run the test command with `src/lib/MarkdownRenderer.test.ts`. Expected: FAIL, with `copy` giving 1 instead of 0.

**Step 3: Implement**

```ts
const SEARCH_EXCLUDED_CLASSES = ['diagram-block', 'diagram-error', 'code-header'];

function isExcludedFromSearch(element: Element): boolean {
    return SEARCH_EXCLUDED_CLASSES.some((name) => element.classList.contains(name));
}
```

Update the call site at line 130, and its comment to "Skip the whole subtree: diagram source and SVG, and code-block headers, are never searchable". Then update the `findMatchesInDom` doc comment to name `code-header`. Grep for any other `isDiagramElement` uses and rename them too.

**Step 4: Run the tests and confirm they pass**

Run the full view suite. Expected: PASS.

**Step 5: Commit**

```bash
git add src/ui/plugins/file-viewer/views/file-viewer/src/lib/search.ts src/ui/plugins/file-viewer/views/file-viewer/src/lib/MarkdownRenderer.test.ts
git commit -m "fix: keep code-block headers out of file viewer search"
```

---

### Task 5b: Headers only on real fences, and indented `<details>` (added 2026-10-31 after the first live check)

**Why.** In the live check, a decoy block written as raw HTML in a document used the reserved
classes. It rendered with a header and a Copy button that looked real and did nothing (the nonce
check made it inert). Ross wants such blocks to render as plain content, with no header and no
button.

Separately, content inside an open `<details>` should be indented.

**Approach.**
- Headers are no longer part of the HTML that goes through the sanitizer.
- The marked code renderer marks its own `<pre>` with the per-render nonce. After `{@html}` renders, a DOM pass adds the header only to `pre[data-copy-nonce]` elements whose value matches the current nonce. It builds the header with DOM APIs.
- The sanitizer strips our reserved class names from all author HTML, so a fake block can never pick up our styling. It also drops `<button>` from markdown entirely (DOMPurify keeps the button's text).
- The click handler's nonce check stays as defence in depth.

**Files:**
- Modify: `VIEW/src/lib/codeCopy.ts`, `VIEW/src/lib/codeCopy.test.ts`
- Modify: `VIEW/src/lib/sanitize.ts`, `VIEW/src/lib/sanitize.test.ts`
- Modify: `VIEW/src/lib/MarkdownRenderer.svelte`, `VIEW/src/lib/MarkdownRenderer.test.ts`
- Modify: `VIEW/src/lib/markdown.css`

**Acceptance Criteria:**
1. `codeCopy.ts` exports `CODE_COPY_RESERVED_CLASSES`, one list holding `code-block`, `code-header`, `code-language`, `code-copy`, `code-copy-icon`, `code-copy-label` and `code-copy-status`. `sanitize.ts` imports it; nothing else duplicates the list.
2. `sanitizeMarkdownHtml`:
   - removes every reserved class from any element and keeps the element's other classes, dropping the `class` attribute when it ends up empty;
   - forbids `button`, keeping its text content;
   - uses a markdown-only DOMPurify instance or a hook added and removed around the call, so `sanitizeSvg` and any other DOMPurify user is unaffected.
3. The code renderer emits `<pre data-copy-nonce="NONCE" data-code-language="LANG">` (both escaped; omit `data-code-language` when the fence has no language) around the unchanged `<code class="hljs ...">`. It no longer calls any header builder.
4. `codeCopy.ts` exports `attachCodeHeaders(root: HTMLElement, nonce: string): void`. It wraps each `pre[data-copy-nonce]` under `root` whose value equals `nonce` in `div.code-block`, inserting a header built with `document.createElement`:
   - `textContent` for the language;
   - the button with `type="button"`, `data-copy-state="idle"`, `data-copy-nonce`, `aria-label` and `title` set to "Copy code", and its fixed `CONTENT.idle` markup;
   - the empty `role="status"` live region.

   It is idempotent: an already-wrapped `pre` is skipped. A `pre` whose nonce does not match is left untouched. `codeHeaderHtml` is removed.
5. `MarkdownRenderer.svelte` calls `attachCodeHeaders(container, currentCopyNonce)` from an `$effect` that depends on `parsedHtml` and `container`. The effect is declared before the search-highlighting effect, so headers exist before marks are computed. The diagram effect is unaffected.
6. A raw-HTML decoy (`<div class="code-block"><div class="code-header"><span class="code-language">bash</span><button class="code-copy">Copy</button></div><pre><code>echo DECOY</code></pre></div>`) renders:
   - with no element having any reserved class;
   - with no `button`;
   - with no header;
   - and its text still visible.
7. A raw `<pre data-copy-nonce="guess"><code>x</code></pre>` gets no header.
8. Real fences still get exactly one header each. All existing copy, feedback, status, round-trip, search-mark and search-exclusion tests pass, updated only where they referenced `codeHeaderHtml`.
9. CSS:
   - `.markdown-renderer details > :not(summary)` gets `margin-left: 1.25em`;
   - `.markdown-renderer details` gets `margin-bottom: 0.8em`;
   - `.markdown-renderer summary` gets `cursor: pointer`;
   - theme tokens only where colors are involved (none needed here).
10. The full view suite and `pnpm build` pass. Commit: `fix: add code headers only to real fences and strip reserved classes from author markup`. The `<details>` CSS may be its own commit: `style: indent the contents of open details blocks in qv`.

---

### Task 5c: Views receive only the events they subscribe to (added 2026-10-31 after the copy failures)

**Root cause (proven).** The copy button failed about one click in ten with `NotAllowedError`.
- WebKitGTK's `webkit_web_view_evaluate_javascript` always runs with `RemoveTransientActivation::Yes` (`Source/WebKit/UIProcess/API/glib/WebKitWebView.cpp:4500`, tag `webkitgtk-2.54.1`). So every host push into a page wipes the page's user activation, and `navigator.clipboard.writeText` requires that activation.
- The bridge forwards every channel to a view that has not subscribed to anything (`subscriptions.rs`, the `None` state). qv subscribes to nothing, so it received `system.stats.event` every second, plus Hyprland, power and notification events.
- A diagnostic build logged the sequence for a failing click: activation `true` at mousedown (13:24:26.449), a host `system.stats.event` push at 26.524, activation `false` at 26.548, and the click at 26.567 refused. The first failing click straddled the same once-a-second push.

**Change.** Remove the forward-everything state. Each webview's subscription set starts empty and only grows through `bridge.subscribe`. `theme.reloaded` stays always-forwarded. The injected plugin-client bootstrap (used by third-party pages such as `~/.config/quantum/plugins/moon-distance`) now registers its channels, the way `@quantum/client` already does.

**Files:**
- Modify: `src/ui/host/src/subscriptions.rs` (the type, `should_forward`, and the tests)
- Modify: `src/ui/host/src/bridge.rs` (subscribe and unsubscribe handling; remove the forward-everything comment)
- Modify: `src/ui/host/src/windows/widget.rs:573` and `src/ui/host/src/windows/panel.rs:277` (construct an empty set)
- Modify: `src/ui/host/src/scheme.rs` (`PLUGIN_CLIENT_SCRIPT` subscribe and unsubscribe, plus a test)

**Acceptance Criteria:**
1. `WebviewSubscriptions` is `Arc<Mutex<HashSet<String>>>`, with no `Option`. `should_forward(channel, &HashSet<String>)` returns true for `theme.reloaded` and for listed channels only. The doc comments state the rule and why, citing the activation finding.
2. Tests in `subscriptions.rs`:
   - an empty set forwards nothing except `theme.reloaded`;
   - a set forwards exactly its listed channels;
   - the old "not seeded forwards anything" test is deleted.
3. Every construction site builds an empty set. A workspace grep for `Option<HashSet<String>>` in the host finds nothing related to subscriptions.
4. `bridge.rs`: `bridge.subscribe` inserts and `bridge.unsubscribe` removes. The comments no longer mention a forward-all state.
5. `PLUGIN_CLIENT_SCRIPT`'s `subscribe(channel, callback)`:
   - posts `bridge.subscribe` with `{ channel }` through its existing `call` path when the first callback for that channel is added;
   - posts `bridge.unsubscribe` when the last callback is removed, counting callbacks per channel;
   - swallows rejection, like `@quantum/client`'s `signalBridge`.
   
   A Rust unit test asserts the script contains both method names. Behaviour is proven live in Task 7: the moon widget still updates.
6. `cargo test -p quantum-ui` (or the host crate's actual package name), `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets -- -D warnings` pass. The frontend suites are unaffected.
7. Commit: `fix: send each view only the events it subscribes to`.

**Live checks (Task 7):**
- Over 30 or more Copy clicks across blocks in qv, none fails.
- The bar's meters, active-window title, timer ring and notifications still update.
- The task manager and file explorer still receive their events.
- The `moon-distance` widget still renders and updates.

---

### Task 7: Live verification in a dev daemon

The tests cannot prove WebKit behaviour: the policy's real source list, host-injected scripts under the policy, actual clipboard contents, and selection copy. This task proves them on the real engine.

**This task needs Ross.** Stopping `quantum.service` removes his bar, launcher and notifications until it is restarted. Tell him first, and wait for him to be ready. The clicks in Step 5 are his: the agent cannot click in his windows. The agent reads the results with `wl-paste` and the log.

**Files:**
- Create (scratch, not committed), all in `/tmp/opencode/qv-live/`:
  - `probe.md` (the 2026-10-31 probe, unchanged);
  - `showcase.md`;
  - `local.png`;
  - `sample.json`;
  - `sample.rs`;
  - `clip.mp4`.

**Acceptance Criteria:**
1. `probe.md` shows its sentence unchanged ("If you can read this sentence unchanged, the script did not run.").
2. Each fixture opens and renders as before with the dev daemon:
   - `showcase.md`, which contains a mermaid fence, a Graphviz fence, a relative local image, an `https` image, `<details>`, and three code blocks (indented, no language, and a wide line);
   - `sample.json`;
   - `sample.rs`;
   - `local.png`;
   - `clip.mp4`;
   - `qv --diff HEAD~1..HEAD` in the quantum repository.
3. `/tmp/quantum-dev.log` contains no policy violations except the expected inline bootstrap one (at most one per page load). Search it with `grep -i -E "content security policy|refused to"`.
4. Opening a file with `qv` still works (this is `__quantum_args` injection under the policy). Ctrl+F still works, and the window's close button works.
5. For each of the three code blocks in `showcase.md`, clicking Copy then running `wl-paste` gives exactly the block's text. The indented block keeps its indentation.
6. Selecting text from the middle of one code block into the next and pressing Ctrl+C pastes no "Copy" or language text.
7. Ctrl+F for `copy` on `showcase.md` reports only matches in the prose (count them in the fixture first).

**Step 1: Build**

```bash
./scripts/devsh.sh bash -c "cd src/ui/plugins/file-viewer/views/file-viewer && pnpm build"
./scripts/devsh.sh cargo build --bin quantumd
```

**Step 2: Write the fixtures**

- `showcase.md`: one prose sentence containing the word "copy" exactly once, plus everything listed in criterion 2. The Graphviz fence must include `a [URL="javascript:alert(1)"]`.
- `local.png`: any small PNG, for example from `grim -g "0,0 64x64"`.
- `clip.mp4`: `ffmpeg -f lavfi -i testsrc=duration=2:size=320x240:rate=10 /tmp/opencode/qv-live/clip.mp4`, if ffmpeg is available. Otherwise, find any existing short mp4.

**Step 3: Swap in the dev daemon (only after Ross says go)**

```bash
systemctl --user stop quantum.service
systemd-run --user --unit=quantum-dev --working-directory="$PWD" \
  --setenv=RUST_LOG=info --setenv=QUANTUM_LAYER_SHELL=1 --setenv=QUANTUM_INSPECTOR=1 \
  --setenv=QUANTUM_PLUGIN_DIR="$PWD/src/ui/plugins" \
  bash -c './scripts/devsh.sh ./target/debug/quantumd > /tmp/quantum-dev.log 2>&1'
```

**Step 4: Open each fixture and check the log**

Open each fixture with `qv <absolute-path>`. After each one, run the `grep` from criterion 3 on `/tmp/quantum-dev.log`. Screenshot each window with `grim -g` (find the geometry with `hyprctl clients -j`) and read the image. Record each result as pass or fail.

**If a violation other than the bootstrap appears:**
- Read which directive and which source it names.
- Add only that source to the matching directive in `index.html`. For example, if `'self'` does not match `quantum://plugin` for scripts, add `quantum:` to `script-src`.
- Rebuild the view, reopen, and record why in the HTML comment.
- Rerun the Task 4 test, then commit the change with `fix: allow <source> in the file viewer policy (<reason>)`.

**Step 5: Copy checks with Ross, one case at a time**

1. Ask Ross to click Copy on block 1.
2. Run `wl-paste | od -c | head` and compare against the fixture byte for byte.
3. Repeat for blocks 2 and 3, then do the selection check from criterion 6.
4. Give Ross a plain readout after each case.

**Step 6: Restore the installed daemon**

```bash
systemctl --user stop quantum-dev
systemctl --user start quantum.service
```

Expected: `systemctl --user is-active quantum.service` prints `active`, and the bar is back.

---

### Task 8: Docs, then deploy to the installed qv

**Files:**
- Modify: `AGENTS.md`, the "File viewer" bullet. Add a short paragraph covering:
  - markdown HTML goes through `lib/sanitize.ts`, and Graphviz SVG through `sanitizeSvg`;
  - the page has a Content Security Policy in `index.html`, with the one expected bootstrap violation;
  - fenced code gets a header strip with a copy button (`lib/codeCopy.ts`);
  - `code-header` is excluded from search;
  - qv is already a secure context, so `navigator.clipboard` works.
- Modify: `docs/plans/2026-10-31-qv-markdown-sanitize-and-code-copy-design.md`: set the status line to shipped, with the date.

**Acceptance Criteria:**
1. `AGENTS.md` describes all five points above in plain words, with no abbreviations.
2. The quantum commits are pushed to `origin/main`.
3. The config repository's quantum flake input is bumped and `just build` succeeds, following the `nixos-config-change` skill.
   - The same config change adds glib-networking to the quantum daemon's wrapper, with `--prefix GIO_EXTRA_MODULES : ${pkgs.glib-networking}/lib/gio/modules`.
   - Root cause, proven live on 2026-10-31: without it, GLib has no TLS backend, so WebKit fails every `https` request from every quantum view. Plain `http` images loaded. `https` images loaded only after the dev daemon ran with glib-networking added to `GIO_EXTRA_MODULES`.
   - Verify after the switch: `tr '\0' '\n' < /proc/<quantumd pid>/environ | grep GIO_EXTRA_MODULES` lists glib-networking, and an `https` image renders in `qv`.
4. Ross runs `just switch`. The agent never does.
5. After the switch:
   - `grep -a` for `sanitizeMarkdownHtml` or `code-copy` in the new store path's `bin/.quantumd-wrapped` finds the string (see the quantum `AGENTS.md` note about the two wrappers);
   - `probe.md` opened with the installed `qv` no longer runs;
   - a code block copies correctly.
6. The exocortex inbox item "quantum: markdown in qv can run commands as the user" is updated: the qv path is fixed, and the per-view IPC allowlist remains open. Mark it done only if Ross agrees the allowlist is tracked separately.

**Steps:**
1. Edit the docs.
2. Commit with `docs: record markdown sanitizing, the policy and the code copy button`.
3. Push.
4. Load the `nixos-config-change` skill and follow it through `just build`.
5. Hand off `just switch` to Ross.
6. Verify the five checks in criterion 5.
