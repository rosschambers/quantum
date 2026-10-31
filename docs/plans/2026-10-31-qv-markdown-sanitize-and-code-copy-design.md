# qv: sanitize rendered markdown, then add a code-block copy button

Status: shipped 2026-10-31 (config `259496c`, quantum `0e28528a`). Verified on the installed build: the probe no longer runs, `https` images load, and code-block copies are byte-exact with no failures.

## Why

Ross asked for a copy button on code blocks in qv's rendered markdown. Checking whether the
clipboard API was safe to use turned up a security hole.

**The hole.** qv runs JavaScript that is embedded in a markdown file.

- `MarkdownRenderer.svelte` passes `marked.parse(content)` straight into `{@html}`. marked 14
  passes raw HTML through: `<img src=x onerror="...">` comes out untouched.
- Confirmed live on 2026-10-31 (WebKitGTK 2.54.1). A probe file's `onerror` handler ran in qv
  and wrote into its own page.
- The custom heading, inline-code and image renderers also splice text and attributes in
  without escaping.
- Graphviz SVG is inserted with `innerHTML` unsanitized, so dot `URL=` attributes become
  links inside the SVG.
- There is no Content Security Policy on any `quantum://` page.

**Why it matters.** From code reading (not exploited live), the bridge in
`src/ui/host/src/bridge.rs` forwards every method to the global dispatcher with no per-view
allowlist. `action.invoke` on the `shell` provider runs `sh -c`. So a markdown file opened in
qv can run commands as Ross, including any file an agent writes and opens for review.

**The clipboard question is settled.** The same live probe reported:

- `isSecureContext=true`;
- `navigator.clipboard` is an object, and `writeText` is a function;
- `crypto.randomUUID` is a function;
- the page's origin is `quantum://plugin`.

WebKit 2.38 and later treats any app-registered scheme as potentially trustworthy, so
`quantum://` is already a secure context. The copy button therefore needs no host change.

Clipboard reads are not a concern. WebKitGTK refuses reads of foreign clipboard content unless
the host approves a `permission-request` signal, and quantum connects no handler for it.
`http://` images are unaffected, because WebKit's mixed-content checks apply only to `https`
documents.

## Decisions and the alternatives rejected

| Decision | Chosen | Rejected, and why |
|---|---|---|
| Fix scope | Sanitize plus a Content Security Policy, in qv only | **Sanitize only**: no second guard, so the next bug of the same kind is exploitable again. **Also add per-view IPC allowlists**: the real fix for "any view can run commands", but a Rust architecture change across every plugin descriptor. It stays a separate inbox item, not part of this work. |
| Raw HTML in markdown | Keep a safe subset (DOMPurify) | **Strip all raw HTML**: Ross's docs use `<details>`, `<summary>`, `<br/>`, spans and tables, which would show as literal tags. |
| Clipboard route | `navigator.clipboard.writeText` from the click | **Daemon `action.invoke` to `wl-copy`**: an extra IPC round trip through a provider meant for another purpose. **Register `quantum://` as secure in `scheme.rs`**: redundant, since the live probe showed it is already secure. **`document.execCommand('copy')`**: deprecated. |
| Button look | Header strip (chosen in the playground) | **Corner button**, hidden at rest or always visible: it can cover the end of a long first line, and when hidden it is not discoverable. The header strip never overlaps code; the cost is about 28px of height per block. |

Playground: `docs/playgrounds/2026-10-31-markdown-code-copy-button.html`, preset "Header strip".

## Part 1: close the hole

### Components

1. **`lib/sanitize.ts`** (new, pure).
   - `sanitizeMarkdownHtml(html)` wraps DOMPurify (already in the dependency tree via mermaid;
     add it as a direct dependency) with a fixed configuration:
     - the default HTML profile;
     - `FORBID_TAGS: ['style', 'form']` and `FORBID_ATTR: ['name']`;
     - URLs limited to `http:`, `https:`, `mailto:`, `file:` and relative references. qv's
       relative images resolve to `file://`, which DOMPurify's default list would strip.
       DOMPurify still allows `data:` images, and the navigation policy already blocks clicks
       on `file:` links;
     - `SANITIZE_DOM: false`. DOMPurify would otherwise strip heading ids such as `images`
       or `title`, which shadow `document` properties, and that breaks the table of
       contents. Clobbering via `name` is handled by forbidding the attribute.
   - `sanitizeSvg(svg)` wraps DOMPurify's SVG profile, with no links that use `javascript:`.
2. **`MarkdownRenderer.svelte`**:
   - `parsedHtml` becomes `sanitizeMarkdownHtml(marked.parse(content))`.
   - The heading renderer escapes `token.text` for its `id`.
   - The image renderer escapes `src`, `alt` and `title`.
   - Graphviz output goes through `sanitizeSvg` before `placeholder.innerHTML = svg`.
   - Mermaid already sanitizes in `securityLevel: 'strict'`.
   - Remove the `crypto.randomUUID` fallback and its wrong "may not be a secure context"
     comment.
3. **`index.html`**: add a `<meta http-equiv="Content-Security-Policy">` tag.
   - Starting point:
     - `default-src 'self'`
     - `script-src 'self' 'wasm-unsafe-eval'`
     - `style-src 'self' 'unsafe-inline'`
     - `img-src 'self' file: data: blob: https: http:`
     - `media-src 'self' file: blob:`
     - `font-src 'self' data:`
     - `connect-src 'self'`
     - `object-src 'none'`
     - `base-uri 'none'`
     - `form-action 'none'`
     - `frame-src 'none'`
   - The final source list is settled by exercising every renderer under the policy, not
     guessed. In particular, check whether `'self'` matches `quantum://plugin` or the scheme
     must be listed as `quantum:`.

### Acceptance criteria

- Unit tests cover a markdown fixture containing each of these, and none of them is active
  after rendering:
  - `<img onerror>`
  - `<svg onload>`
  - a `javascript:` link
  - `<script>`
  - `<style>`
  - `<form>`
  - an image title that tries to break out of its attribute
  - a heading containing HTML
- A Graphviz fence with `URL="javascript:..."` renders without that link.
- `<details>`, `<summary>`, `<br>`, `<kbd>`, tables and spans still render.
- Live, in a dev daemon:
  - the 2026-10-31 probe file no longer runs (its sentence stays unchanged);
  - each kind of file opens with no policy violation and no behaviour change: markdown with
    mermaid, Graphviz, a local image and an `https` image, JSON, code, an image, a video, and
    diff mode.
- Host-injected JavaScript still works under the policy: `__quantum_args`,
  `__quantum_view_name`, `__quantum_notify`, and request replies. This rests on the
  assumption that `evaluate_javascript` is not subject to the page's policy, so it is checked
  live.

## Part 2: the copy button (header strip)

### Components

1. **Header strip.** The markdown code renderer wraps each fenced, non-diagram block:
   - Structure: `<div class="code-block"><div class="code-header">…</div><pre><code class="hljs">…</code></pre></div>`.
   - The header holds the fence's language name on the left (empty when the fence names none)
     and the button on the right.
   - The header is always visible. The `<pre>` loses its top border and top radius under it.
2. **`lib/codeCopy.ts`** (new). One delegated click handler on the markdown container:
   - It copies the sibling `code` element's `textContent`, which is exactly the fenced source,
     with `navigator.clipboard.writeText`.
   - On success, the copy icon becomes a check and the text reads "Copied" in
     `--color-accent`, for 1.5 seconds.
   - On failure (a rejected promise or a missing API), an alert icon and "Copy failed" show in
     `--color-error` for the same time.
   - The button's footprint never shrinks while it shows feedback.
3. **Button styling.**
   - Ghost style (no fill; hover fills `--color-surface-hover`), 24px tall, copy icon plus the
     word "Copy".
   - `aria-label` and `title` are both "Copy code".
   - Header text uses `--font-mono` and `--color-muted`.
   - Theme tokens only. The icons are inline SVG, never emoji.
4. **Hygiene.**
   - `search.ts` skips `.code-header`, the way it skips diagram blocks, so the language name
     and "Copy" never count as matches.
   - The header is `user-select: none`, so selecting text across blocks copies only code.

### Acceptance criteria

- Live: clicking a block's button puts exactly that block's code on the clipboard. Checked with
  `wl-paste` for:
  - a block with leading indentation;
  - a block with no language;
  - a wide block that scrolls sideways.
- Ctrl+F match counts on a document with code blocks are unchanged by the headers.
- A selection spanning two blocks copies no header text.
- With `navigator.clipboard.writeText` stubbed to reject, the button shows "Copy failed".
- Diagram fences (mermaid, Graphviz) get no header.

## Assumptions

- WebKitGTK 2.54.1, as loaded by the running daemon on 2026-10-31.
- DOMPurify 3.4.x and marked 14.
- `evaluate_javascript` from the host is not blocked by a page Content Security Policy. This
  is verified live in part 1.

## Out of scope

- Per-view IPC method allowlists. These are tracked in the exocortex inbox ("quantum: markdown
  in qv can run commands as the user").
- Copy buttons in the whole-file code view, the JSON view and diff mode.
