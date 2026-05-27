# Theming Guide

Quantum themes are a collection of TOML configuration files and Svelte 5 view bundles. Each theme defines design tokens (colors, fonts, spacing) and provides HTML/CSS/JS implementations for views like the launcher.

## Theme Structure

A minimal theme lives in `frontend/themes/<name>/`:

```
frontend/themes/my-theme/
├── theme.toml                      # manifest
├── tokens.toml                     # design token definitions
└── views/
    └── launcher/                   # one view per directory
        ├── package.json
        ├── vite.config.ts
        ├── index.html
        ├── src/
        │   ├── main.ts
        │   └── App.svelte
        └── dist/                   # built output (generated)
            ├── index.html
            └── assets/
```

---

## Theme Manifest (theme.toml)

Declare the theme's name, extends chain, and content sources.

```toml
[theme]
name = "my-theme"
version = "1.0.0"
description = "A custom theme"

# Extends another theme's tokens and views (optional)
extends = "default"                 # chain depth max = 8

# Token source
[tokens]
source = "file"                     # "file" or "inline"

# View definitions
[[views]]
name = "launcher"
description = "Application launcher"
```

### Cascade and Inheritance

A theme can extend another theme, creating a cascade:

```toml
extends = "default"
```

When resolving tokens:
1. Start with the root theme (default)
2. Walk the `extends` chain (with cycle detection)
3. Apply overrides in dependency order (root → leaf)

**Example cascade:**

```
my-theme → extends: dark-mode → extends: default
```

Resolution order:
1. Load default tokens
2. Merge dark-mode tokens
3. Merge my-theme tokens
```

---

## Token Definitions (tokens.toml)

Define design tokens as flat key-value pairs.

```toml
# Colors
color-bg = "#0a0e27"
color-fg = "#ffffff"
color-fg-muted = "#888888"
color-accent = "#6366f1"
color-border = "#333333"

# Typography
font-sans = "system-ui, sans-serif"
font-mono = "monospace"
font-size-base = "14px"
font-size-lg = "16px"
font-size-sm = "12px"

# Spacing (use a scale)
space-1 = "4px"
space-2 = "8px"
space-3 = "12px"
space-4 = "16px"
space-5 = "20px"
space-6 = "24px"

# Radii
radius-sm = "2px"
radius-md = "4px"
radius-lg = "8px"

# Motion
duration-fast = "150ms"
duration-base = "300ms"
ease-default = "cubic-bezier(0.4, 0, 0.2, 1)"
```

Token names follow kebab-case convention. The daemon injects them as CSS custom properties at runtime:

```css
background: var(--color-bg);
color: var(--color-fg);
border-radius: var(--radius-md);
```

---

## View Bundles

Each view (e.g., launcher) is a self-contained Vite project that builds to a static bundle.

### Vite Configuration

```typescript
// vite.config.ts
import { defineConfig } from 'vite'
import { svelte } from 'vite-plugin-svelte'

export default defineConfig({
  plugins: [svelte()],
  base: 'quantum://theme/my-theme/views/launcher/',
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    rollupOptions: {
      output: {
        entryFileNames: 'assets/[name].js',
        chunkFileNames: 'assets/[name].js',
        assetFileNames: 'assets/[name][extname]'
      }
    }
  }
})
```

### HTML Template

```html
<!-- index.html -->
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Quantum Launcher</title>
  <style id="quantum-tokens"></style>
  <link rel="stylesheet" href="styles.css">
</head>
<body>
  <div id="app"></div>
  <script type="module" src="/src/main.ts"></script>
</body>
</html>
```

The `<style id="quantum-tokens"></style>` element is populated by the daemon with CSS custom property declarations.

### Svelte Component Example

```svelte
<!-- src/App.svelte -->
<script lang="ts">
  import { createClient } from '@quantum/client'
  
  const client = createClient()
  let query = ''
  let results = []
  let selectedIndex = 0
  
  async function search() {
    const { matches, warnings } = await client.call('search', {
      text: query,
      providers: [],
      limit: 20
    })
    results = matches
    selectedIndex = 0
  }
  
  async function invoke() {
    if (results[selectedIndex]) {
      const match = results[selectedIndex]
      await client.call('action.invoke', {
        provider_id: match.provider,
        action: match.action
      })
      // Close launcher
      await client.call('view.hide', { name: 'launcher' })
    }
  }
  
  function handleKeydown(e) {
    if (e.key === 'ArrowDown') {
      selectedIndex = (selectedIndex + 1) % results.length
    } else if (e.key === 'ArrowUp') {
      selectedIndex = (selectedIndex - 1 + results.length) % results.length
    } else if (e.key === 'Enter') {
      invoke()
    } else if (e.key === 'Escape') {
      client.call('view.hide', { name: 'launcher' })
    }
  }
</script>

<input
  type="text"
  placeholder="Search..."
  bind:value={query}
  on:input={search}
  on:keydown={handleKeydown}
  autofocus
/>

<ul>
  {#each results as result, i}
    <li class:selected={i === selectedIndex}>
      {result.title}
      {#if result.subtitle}
        <small>{result.subtitle}</small>
      {/if}
    </li>
  {/each}
</ul>

<style>
  :global(body) {
    background: var(--color-bg);
    color: var(--color-fg);
    font-family: var(--font-sans);
  }
  
  input {
    width: 100%;
    padding: var(--space-3);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
  }
  
  ul {
    list-style: none;
    padding: 0;
  }
  
  li {
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius-sm);
  }
  
  li.selected {
    background: var(--color-accent);
    color: white;
  }
</style>
```

---

## Building a View Bundle

```bash
cd frontend/themes/my-theme/views/launcher
pnpm build
```

Output goes to `dist/`, with `index.html` and hashed asset imports.

---

## Hot Reload During Development

While developing, use `quantum-dev watch`:

```bash
quantum-dev watch
```

This watches `frontend/themes/` and pushes `theme.reload` to the running daemon whenever files change. The frontend reloads the view.

---

## Built-in Default Theme

The `default` theme is embedded in the binary via `include_dir!`. Its structure:

```
frontend/themes/default/
├── theme.toml              (name="default", no extends)
├── tokens.toml             (standard Quantum design tokens)
└── views/launcher/
    └── dist/               (pre-built Svelte bundle)
```

Builds happen at daemon compile time:

```bash
# In crates/infrastructure/build.rs
cargo run -C ../../frontend/themes/default/views/launcher build
# Then include_dir! packs the `dist/` folder
```

---

## Extending the Default Theme

Create a new theme that inherits from default and overrides tokens:

```toml
# frontend/themes/dark/theme.toml
[theme]
name = "dark"
extends = "default"

[tokens]
source = "file"
```

```toml
# frontend/themes/dark/tokens.toml
# Only override what you need
color-bg = "#0a0a0a"
color-fg = "#ffffff"
color-accent = "#ff6b9d"
```

The daemon merges these over the default tokens.

---

## Custom Views

Create a new view (e.g., widgets) by adding to a theme:

```toml
[[views]]
name = "widgets"
description = "Floating widgets"
```

Then implement `frontend/themes/<name>/views/widgets/`:

```bash
mkdir -p frontend/themes/my-theme/views/widgets
cd frontend/themes/my-theme/views/widgets

# Copy structure from launcher
cp -r ../launcher/* .

# Edit App.svelte to show widgets instead
```

Build and activate via `quantumctl view.show widgets`.

---

## Cycle Detection

If a theme's `extends` chain forms a cycle, the daemon logs an error and falls back to the default theme:

```
Error loading theme: cycle detected in extends chain (depth > 8)
Falling back to default theme
```

---

## Token Injection

When a view is loaded, the daemon injects CSS custom properties into the `<style id="quantum-tokens">` element:

```css
--color-bg: #0a0e27;
--color-fg: #ffffff;
--font-sans: system-ui, sans-serif;
--space-4: 16px;
...
```

The view's CSS references them:

```css
body {
  background: var(--color-bg);
  color: var(--color-fg);
  font-family: var(--font-sans);
  padding: var(--space-4);
}
```

---

## Troubleshooting

**Theme fails to load:**

```bash
# Check the log
RUST_LOG=quantum_infrastructure=debug quantumd --headless
```

**CSS variables not applied:**

- Ensure tokens are defined in `tokens.toml`
- Check the browser console for CSS variable errors
- Verify `<style id="quantum-tokens">` is populated

**Hot reload not working:**

```bash
quantum-dev watch --verbose
# Check that daemon is running and the socket is accessible
```

---

## See Also

- [docs/protocol.md](protocol.md) — `theme.reload` method
- [docs/architecture.md](architecture.md) — ThemeStore cascade implementation
