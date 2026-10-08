<script lang="ts">
    import { marked } from 'marked';
    import { escapeHtml, highlightCode } from './highlighter';
    import { isMermaidLanguage, isGraphvizLanguage, diagramPlaceholderHtml, type DiagramRenderer } from './mermaid';
    import { findMatchSegmentsInDom, type MatchSegment } from './search';
    import { measureFractions, observeLayout } from './measureOffsets';
    import { slugify } from './types';
    import './markdown.css';

    let mermaidInitialized = false;
    let mermaidRenderCounter = 0;

    // crypto.randomUUID requires a secure context, which the quantum:// custom
    // scheme may not provide; fall back to a counter for the render id.
    function mermaidRenderId(): string {
        return `mermaid-${crypto.randomUUID?.() ?? String(++mermaidRenderCounter)}`;
    }

    // The Graphviz WASM module is heavier than mermaid and only loads for files
    // that actually contain dot/digraph/graphviz fences. Mermaid cannot parse
    // DOT syntax, so those languages get their own renderer. `Graphviz.load()`
    // boots the WASM instance (cached after the first call); the layout methods
    // live on the instance, not the class.
    let graphvizInstance: { layout: (source: string, format: string) => string } | undefined;

    async function loadGraphviz(): Promise<{ layout: (source: string, format: string) => string }> {
        if (!graphvizInstance) {
            const module = await import('@hpcc-js/wasm-graphviz');
            graphvizInstance = await module.Graphviz.load();
        }
        return graphvizInstance;
    }

    interface Props {
        content: string;
        fileDirectory?: string;
        /** Literal, case-insensitive search query. Empty string means inactive. */
        query?: string;
        /** Index into this component's own match list that is "current". */
        currentMatchIndex?: number | null;
        /** Fired whenever the computed match count changes. */
        onMatchCount?: (count: number) => void;
        /**
         * Fired whenever the match positions change (never when only
         * currentMatchIndex changes). Positions are fractions along the
         * scroll track, in the same order as onMatchCount's count, for the
         * shared overview ruler.
         */
        onMatchPositions?: (positions: Float64Array) => void;
        navigationRevision?: number;
    }

    let { content, fileDirectory, query = '', currentMatchIndex = null, onMatchCount, onMatchPositions, navigationRevision = 0 }: Props = $props();

    function isAbsoluteOrDataUrl(url: string): boolean {
        return /^(?:[a-z][a-z0-9+.-]*:|\/\/)/i.test(url);
    }

    function resolveImageSrc(href: string): string {
        if (!fileDirectory || !href || isAbsoluteOrDataUrl(href)) {
            return href;
        }
        return `file://${fileDirectory}/${href}`;
    }

    function renderDiagramError(placeholder: HTMLElement, renderer: DiagramRenderer, message: string): void {
        const source = placeholder.textContent ?? '';
        placeholder.classList.add('diagram-error');
        placeholder.removeAttribute('data-diagram-status');
        placeholder.innerHTML = `<p class="diagram-error-message">${escapeHtml(message)}</p><pre><code class="hljs">${highlightCode(source, renderer === 'mermaid' ? 'mermaid' : 'dot')}</code></pre>`;
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
                    return diagramPlaceholderHtml(token.text, 'mermaid');
                }
                if (isGraphvizLanguage(language)) {
                    return diagramPlaceholderHtml(token.text, 'graphviz');
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

    function errorMessage(error: unknown): string {
        return error instanceof Error ? error.message : String(error);
    }

    // Staleness contract: a content re-render replaces the {@html} DOM mid-flight,
    // so the cleanup cancel flag stops a superseded effect run from doing any work,
    // and every placeholder's isConnected is re-checked after each await before its
    // SVG is injected. Renders run sequentially within a pass — mermaid's renderer
    // must not be re-entered concurrently.
    $effect(() => {
        if (!parsedHtml || !container) return;
        const placeholders = Array.from(
            container.querySelectorAll<HTMLElement>('.diagram-block[data-diagram-status="pending"]'),
        );
        if (placeholders.length === 0) return;

        let cancelled = false;

        void (async () => {
            // Each renderer lazy-loads only when a placeholder actually claims it,
            // and a failed load surfaces on that renderer's placeholders instead of
            // leaving them silently stuck.
            const mermaidPlaceholders = placeholders.filter(
                (placeholder) => placeholder.dataset.diagramRenderer === 'mermaid',
            );
            const graphvizPlaceholders = placeholders.filter(
                (placeholder) => placeholder.dataset.diagramRenderer === 'graphviz',
            );

            let mermaid: typeof import('mermaid').default | undefined;
            if (mermaidPlaceholders.length > 0) {
                try {
                    mermaid = (await import('mermaid')).default;
                    const styles = getComputedStyle(document.documentElement);
                    const cssVariable = (name: string, fallback: string) =>
                        styles.getPropertyValue(name).trim() || fallback;

                    if (!mermaidInitialized) {
                        mermaid.initialize({
                            startOnLoad: false,
                            securityLevel: 'strict',
                            theme: 'base',
                            themeVariables: {
                                fontFamily: cssVariable('--font-sans', 'system-ui'),
                                primaryColor: cssVariable('--color-surface', '#f0f2f5'),
                                primaryTextColor: cssVariable('--color-fg', '#1a1a1a'),
                                lineColor: cssVariable('--color-accent', '#5b6770'),
                            },
                        });
                        mermaidInitialized = true;
                    }
                } catch (error) {
                    console.error('Mermaid failed to load:', error);
                    if (!cancelled) {
                        for (const placeholder of mermaidPlaceholders) {
                            if (placeholder.isConnected) {
                                renderDiagramError(placeholder, 'mermaid', `Mermaid failed to load: ${errorMessage(error)}`);
                            }
                        }
                    }
                }
            }

            let graphviz: { layout: (source: string, format: string) => string } | undefined;
            if (graphvizPlaceholders.length > 0) {
                try {
                    graphviz = await loadGraphviz();
                } catch (error) {
                    console.error('Graphviz failed to load:', error);
                    if (!cancelled) {
                        for (const placeholder of graphvizPlaceholders) {
                            if (placeholder.isConnected) {
                                renderDiagramError(placeholder, 'graphviz', `Graphviz failed to load: ${errorMessage(error)}`);
                            }
                        }
                    }
                }
            }

            for (const placeholder of placeholders) {
                if (cancelled || !placeholder.isConnected) continue;
                const renderer = placeholder.dataset.diagramRenderer as DiagramRenderer;
                const source = placeholder.textContent ?? '';
                try {
                    let svg: string;
                    if (renderer === 'graphviz') {
                        if (!graphviz) continue;
                        svg = graphviz.layout(source, 'svg');
                    } else {
                        if (!mermaid) continue;
                        svg = (await mermaid.render(mermaidRenderId(), source)).svg;
                    }
                    if (!placeholder.isConnected || cancelled) continue;
                    placeholder.innerHTML = svg;
                    placeholder.removeAttribute('data-diagram-status');
                } catch (error) {
                    console.error(`Diagram render failed (${renderer}):`, error);
                    if (!placeholder.isConnected || cancelled) continue;
                    renderDiagramError(placeholder, renderer, errorMessage(error));
                }
            }

            // A diagram's rendered SVG almost always changes the document's
            // flow height, shifting every match below it — re-measure once
            // the whole render loop has settled rather than leaving the
            // ruler holding positions from before the diagrams existed.
            if (!cancelled) {
                onMatchPositions?.(measureMatchPositions());
            }
        })();

        return () => {
            cancelled = true;
        };
    });

    let matchMarks: HTMLElement[][] = $state.raw([]);

    function clearMarks(marks: HTMLElement[][]): void {
        const parents = new Set<Node>();
        for (const mark of marks.flat()) {
            const parent = mark.parentNode;
            if (!parent) continue;
            parents.add(parent);
            while (mark.firstChild) {
                parent.insertBefore(mark.firstChild, mark);
            }
            parent.removeChild(mark);
        }
        for (const parent of parents) parent.normalize();
    }

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
        // Explicit dependency on parsedHtml (not otherwise read in this
        // effect) so a content/file change re-runs this, not just a query.
        void parsedHtml;
        if (!container) return;

        const { count, segments } = findMatchSegmentsInDom(container, query);
        const marks = wrapSegments(segments, count);
        matchMarks = marks;
        onMatchCount?.(count);
        // Capture this run's owned marks without reading reactive matchMarks.
        return () => clearMarks(marks);
    });

    $effect(() => {
        void navigationRevision;
        for (const [index, marks] of matchMarks.entries()) {
            for (const mark of marks) mark.classList.toggle('search-match-current', index === currentMatchIndex);
        }
        if (currentMatchIndex !== null) matchMarks[currentMatchIndex]?.[0]?.scrollIntoView({ block: 'center' });
    });

    // Markdown has no fixed per-line row height (rendered prose reflows), so
    // match positions come from one batched live-layout read pass instead of
    // arithmetic, anchored on each match's first mark (`matchMarks[i][0]`).
    function measureMatchPositions(): Float64Array {
        if (!container) return new Float64Array(0);
        const anchors = matchMarks.map((marks) => marks[0] ?? container);
        return measureFractions(container, anchors);
    }

    // Scheduled a frame after each search pass (never synchronously inside
    // the pass itself, and never on a mere currentMatchIndex change, since
    // this depends only on matchMarks).
    $effect(() => {
        void matchMarks;
        const frameId = requestAnimationFrame(() => {
            onMatchPositions?.(measureMatchPositions());
        });
        return () => cancelAnimationFrame(frameId);
    });

    // A persistent resize watch on the content root, independent of search
    // passes, so a layout change re-measures the SAME matches without
    // waiting for another keystroke.
    $effect(() => {
        if (!container) return;
        return observeLayout(container, () => {
            onMatchPositions?.(measureMatchPositions());
        });
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

    .markdown-renderer :global(mark.search-match) {
        background: color-mix(in oklab, var(--color-accent) 35%, transparent);
        color: inherit;
        border-radius: 2px;
    }

    .markdown-renderer :global(mark.search-match-current) {
        background: var(--color-accent);
        color: var(--color-bg);
    }
</style>
