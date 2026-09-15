<script lang="ts">
    import { marked } from 'marked';
    import { escapeHtml, highlightCode } from './highlighter';
    import { isMermaidLanguage, mermaidPlaceholderHtml } from './mermaid';
    import { slugify } from './types';
    import './markdown.css';

    let mermaidInitialized = false;
    let mermaidRenderCounter = 0;

    // crypto.randomUUID requires a secure context, which the quantum:// custom
    // scheme may not provide; fall back to a counter for the render id.
    function mermaidRenderId(): string {
        return `mermaid-${crypto.randomUUID?.() ?? String(++mermaidRenderCounter)}`;
    }

    interface Props {
        content: string;
        fileDirectory?: string;
    }

    let { content, fileDirectory }: Props = $props();

    function isAbsoluteOrDataUrl(url: string): boolean {
        return /^(?:[a-z][a-z0-9+.-]*:|\/\/)/i.test(url);
    }

    function resolveImageSrc(href: string): string {
        if (!fileDirectory || !href || isAbsoluteOrDataUrl(href)) {
            return href;
        }
        return `file://${fileDirectory}/${href}`;
    }

    let parsedHtml = $derived.by(() => {
        try {
            // Configure marked with GFM enabled, no breaks
            marked.setOptions({
                gfm: true,
                breaks: false,
            });

            // Set up custom renderer for code blocks to enable syntax highlighting
            const renderer = new marked.Renderer();

            renderer.heading = (token) => {
                const id = slugify(token.text);
                const level = token.depth;
                return `<h${level} id="${id}" class="anchor-heading"><a class="anchor-link" href="#${id}">#</a>${token.text}</h${level}>`;
            };

            renderer.codespan = (token) => {
                return `<code class="inline-code">${token.text}</code>`;
            };

            renderer.code = (token) => {
                const language = token.lang || undefined;
                if (isMermaidLanguage(language)) {
                    return mermaidPlaceholderHtml(token.text);
                }
                const highlightedCode = highlightCode(token.text, language);
                const languageClass = language ? ` language-${language}` : '';
                return `<pre><code class="hljs${languageClass}">${highlightedCode}</code></pre>`;
            };

            renderer.image = (token) => {
                const src = resolveImageSrc(token.href);
                const alt = token.text || '';
                const titleAttribute = token.title ? ` title="${token.title}"` : '';
                return `<img src="${src}" alt="${alt}"${titleAttribute} />`;
            };

            marked.setOptions({ renderer });

            return marked.parse(content);
        } catch (error) {
            console.error('Markdown parsing error:', error);
            return `<p>Error rendering markdown: ${error instanceof Error ? error.message : String(error)}</p>`;
        }
    });
    let container: HTMLDivElement | undefined = $state();

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
                    const { svg } = await mermaid.render(mermaidRenderId(), source);
                    if (!placeholder.isConnected || cancelled) continue;
                    placeholder.innerHTML = svg;
                    placeholder.removeAttribute('data-mermaid-status');
                } catch (error) {
                    if (!placeholder.isConnected || cancelled) continue;
                    const message = error instanceof Error ? error.message : String(error);
                    placeholder.classList.add('mermaid-error');
                    placeholder.removeAttribute('data-mermaid-status');
                    placeholder.innerHTML = `<p class="mermaid-error-message">Mermaid render failed: ${escapeHtml(message)}</p><pre><code class="hljs">${highlightCode(source, 'mermaid')}</code></pre>`;
                }
            }
        })();

        return () => {
            cancelled = true;
        };
    });
</script>

<div class="markdown-renderer" bind:this={container}>
    {@html parsedHtml}
</div>

<style>
    .markdown-renderer {
        padding: 32px;
        font-size: 15px;
        line-height: 1.7;
        color: var(--color-fg-alt, #666);
    }
</style>
