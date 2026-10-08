<script lang="ts">
    import { createClient } from '@quantum/client';
    import type { ViewerFileInfo } from './lib/types';
    import { selfViewName } from './lib/selfName';
    import { resolveViewerShortcut } from './lib/viewerKeymap';
    import { searchMarks, type RulerMark } from './lib/overviewRuler';
    import Header from './lib/Header.svelte';
    import MarkdownRenderer from './lib/MarkdownRenderer.svelte';
    import CodeRenderer from './lib/CodeRenderer.svelte';
    import ImageRenderer from './lib/ImageRenderer.svelte';
    import VideoRenderer from './lib/VideoRenderer.svelte';
    import TextRenderer from './lib/TextRenderer.svelte';
    import TocSidebar from './lib/TocSidebar.svelte';
    import FormatBanner from './lib/FormatBanner.svelte';
    import JsonFoldRenderer from './lib/JsonFoldRenderer.svelte';
    import SearchBar from './lib/SearchBar.svelte';
    import OverviewRuler from './lib/OverviewRuler.svelte';

    let fileInfo: ViewerFileInfo | null = $state(null);
    let isLoading = $state(true);
    let error: string | null = $state(null);
    let path = $state('');
    let markdownContentElement: HTMLElement | undefined = $state(undefined);
    let displayContent: string | null = $state(null);

    // In-file search (Ctrl+F). `searchQuery`/`currentMatchIndex` are the
    // single source of truth; the active renderer is the only thing that
    // actually computes matches (against whatever IT considers "the text",
    // whether that's raw lines, pretty-printed JSON lines, or rendered
    // Markdown DOM text) and reports the count back via `onMatchCount` — see
    // each renderer's own search wiring. This keeps App.svelte a thin
    // orchestrator with no duplicated knowledge of JSON pretty-printing,
    // fold models, or DOM internals.
    let searchOpen = $state(false);
    let searchQuery = $state('');
    let searchFocusSignal = $state(0);
    let totalMatches = $state(0);
    let currentMatchIndex: number | null = $state(null);
    let navigationRevision = $state(0);
    // What the active renderer actually receives: query is '' whenever
    // search is closed, so every renderer's own effect reports 0 matches and
    // clears/restores its highlighting — never left showing stale matches.
    let activeQuery = $derived(searchOpen ? searchQuery : '');

    // The shared overview ruler's search lane. matchPositions comes from
    // whichever renderer is active (onMatchPositions); searchMarks drops it
    // entirely whenever its length no longer matches totalMatches (for
    // example the instant search closes, or between a renderer's own
    // onMatchCount and onMatchPositions calls), so the ruler never shows
    // marks for a stale match set.
    let matchPositions = $state.raw(new Float64Array(0));
    let rulerMarks = $derived(searchMarks(matchPositions, totalMatches, currentMatchIndex));

    // Each text-like renderer binds its own scroll container here so the
    // ruler beside it can track scroll position and thumb geometry.
    // Markdown's scroller is App's own markdownContentElement, above.
    let codeScrollElement: HTMLElement | null = $state(null);
    let jsonScrollElement: HTMLElement | null = $state(null);
    let textScrollElement: HTMLElement | null = $state(null);

    function handleMatchCount(count: number): void {
        totalMatches = count;
        currentMatchIndex = count > 0 ? 0 : null;
    }

    function handleMatchPositions(positions: Float64Array): void {
        matchPositions = positions;
    }

    // Reused by every renderer's ruler: a match/current-match mark jumps
    // straight to that match, reusing the renderer's own existing reveal
    // path (scrollIntoView, fold expansion, ...) by driving the same
    // currentMatchIndex/navigationRevision state a keyboard Enter would.
    function handleMarkActivate(mark: RulerMark): void {
        if (mark.index === undefined) return;
        currentMatchIndex = mark.index;
        navigationRevision++;
    }

    function openSearch(): void {
        searchOpen = true;
        searchFocusSignal++;
    }

    function closeSearch(): void {
        searchOpen = false;
        searchQuery = '';
        totalMatches = 0;
        currentMatchIndex = null;
        matchPositions = new Float64Array(0);
    }

    function nextMatch(): void {
        if (totalMatches === 0) return;
        navigationRevision++;
        currentMatchIndex = currentMatchIndex === null ? 0 : (currentMatchIndex + 1) % totalMatches;
    }

    function previousMatch(): void {
        if (totalMatches === 0) return;
        navigationRevision++;
        currentMatchIndex = currentMatchIndex === null ? totalMatches - 1 : (currentMatchIndex - 1 + totalMatches) % totalMatches;
    }

    const client = createClient();

    let effectiveContent = $derived(displayContent ?? fileInfo?.content ?? '');

    let markdownHeadingCount = $derived.by(() => {
        if (!fileInfo || fileInfo.file_type !== 'markdown') return 0;
        const matches = effectiveContent.match(/^#{1,6}\s+.+$/gm);
        return matches ? matches.length : 0;
    });

    // Reset displayContent when file changes
    $effect(() => {
        if (fileInfo) {
            displayContent = null;
        }
    });

    // Keyboard listener registration and the initial file load both live inside
    // a top-level $effect, not onMount: testing-library's svelte5 adapter does
    // not reliably fire onMount under Svelte 5 runes mode (see AGENTS.md and
    // the design doc's testability-risk note), confirmed empirically by
    // App.test.ts's spike. The effect body itself stays synchronous (an
    // $effect callback cannot return a Promise); the async load runs inside a
    // fire-and-forget inner function guarded by a `cancelled` flag so a
    // superseded/unmounted effect run never writes stale state.
    $effect(() => {
        const args = (window as any).__quantum_args;
        if (!args?.path) {
            error = 'No file path provided';
            isLoading = false;
            return;
        }

        path = args.path;
        let cancelled = false;

        void (async () => {
            try {
                const result = await client.call('file-viewer.read', { path });
                if (cancelled) return;
                fileInfo = result as ViewerFileInfo;
                isLoading = false;
            } catch (err) {
                if (cancelled) return;
                const errorMessage = err instanceof Error ? err.message : String(err);
                error = `Failed to read file: ${errorMessage}`;
                isLoading = false;
            }
        })();

        // Set up keyboard handlers
        const handleKeyDown = (event: KeyboardEvent) => {
            // Ctrl+ArrowUp/Down for markdown heading navigation
            if (event.ctrlKey && (event.key === 'ArrowUp' || event.key === 'ArrowDown') && fileInfo?.file_type === 'markdown' && markdownContentElement) {
                event.preventDefault();
                const headingElements = Array.from(markdownContentElement.querySelectorAll('h1[id], h2[id], h3[id], h4[id], h5[id], h6[id]'));
                if (headingElements.length === 0) return;

                const contentRect = markdownContentElement.getBoundingClientRect();
                if (event.key === 'ArrowDown') {
                    const next = headingElements.find((heading) => heading.getBoundingClientRect().top > contentRect.top + 10);
                    if (next) next.scrollIntoView({ behavior: 'smooth' });
                } else {
                    const previous = headingElements.filter((heading) => heading.getBoundingClientRect().top < contentRect.top - 10);
                    if (previous.length > 0) previous[previous.length - 1].scrollIntoView({ behavior: 'smooth' });
                }
                return;
            }

            const action = resolveViewerShortcut(event);
            if (!action) return;

            if (action.kind === 'next-match' || action.kind === 'previous-match') {
                const target = event.target;
                if (event.defaultPrevented) return;
                if (target instanceof Element && !target.matches('.search-bar .search-input') &&
                    target.closest('button, a[href], input, textarea, select, summary, [contenteditable]:not([contenteditable="false"]), [role="button"], [role="link"], [role="textbox"]')) return;
            }

            switch (action.kind) {
                case 'open-search':
                    event.preventDefault();
                    openSearch();
                    break;
                case 'next-match':
                    // Enter is never intercepted while search is closed —
                    // the resolver only reports "Enter was pressed"; this
                    // caller decides what that means given current UI state.
                    if (!searchOpen) return;
                    event.preventDefault();
                    nextMatch();
                    break;
                case 'previous-match':
                    if (!searchOpen) return;
                    event.preventDefault();
                    previousMatch();
                    break;
                case 'close-search':
                    // Escape closes an open search bar FIRST; only hides the
                    // viewer window when the bar is already closed.
                    if (searchOpen) {
                        closeSearch();
                    } else {
                        client.call('view.hide', { name: selfViewName() }).catch(console.error);
                    }
                    break;
            }
        };

        window.addEventListener('keydown', handleKeyDown);
        return () => {
            cancelled = true;
            window.removeEventListener('keydown', handleKeyDown);
        };
    });
</script>

<div class="app-container">
    {#if isLoading}
        <div class="loading">
            <div class="spinner"></div>
            <p>Loading file...</p>
        </div>
    {:else if error}
        <div class="error">
            <p class="error-title">Error</p>
            <p class="error-message">{error}</p>
        </div>
    {:else if fileInfo}
        <Header {fileInfo} />
        {#if fileInfo}
            <FormatBanner
                content={fileInfo.content}
                fileType={fileInfo.file_type}
                onformat={(formatted) => { displayContent = formatted; }}
            />
        {/if}
        {#if searchOpen}
            <SearchBar
                query={searchQuery}
                {totalMatches}
                {currentMatchIndex}
                onQueryInput={(value) => { searchQuery = value; }}
                onNext={nextMatch}
                onPrevious={previousMatch}
                onClose={closeSearch}
                focusSignal={searchFocusSignal}
            />
        {/if}
        <div class="content-area">
            {#if fileInfo.file_type === 'markdown'}
                <div class="renderer-pane">
                    <div class="markdown-layout" class:has-toc={markdownHeadingCount >= 3}>
                        {#if markdownHeadingCount >= 3}
                            <TocSidebar content={effectiveContent} contentElement={markdownContentElement} />
                        {/if}
                        <div class="markdown-content" bind:this={markdownContentElement}>
                            <MarkdownRenderer
                                content={effectiveContent}
                                fileDirectory={fileInfo?.directory}
                                query={activeQuery}
                                {currentMatchIndex}
                                {navigationRevision}
                                onMatchCount={handleMatchCount}
                                onMatchPositions={handleMatchPositions}
                            />
                        </div>
                    </div>
                </div>
                <OverviewRuler marks={rulerMarks} scrollElement={markdownContentElement} onMarkActivate={handleMarkActivate} />
            {:else if fileInfo.file_type === 'json'}
                <div class="renderer-pane">
                    <JsonFoldRenderer
                        content={effectiveContent}
                        query={activeQuery}
                        {currentMatchIndex}
                        {navigationRevision}
                        onMatchCount={handleMatchCount}
                        onMatchPositions={handleMatchPositions}
                        bind:scrollElement={jsonScrollElement}
                    />
                </div>
                <OverviewRuler marks={rulerMarks} scrollElement={jsonScrollElement} onMarkActivate={handleMarkActivate} />
            {:else if fileInfo.file_type === 'code'}
                <div class="renderer-pane">
                    <CodeRenderer
                        content={effectiveContent}
                        language={fileInfo.language}
                        query={activeQuery}
                        {currentMatchIndex}
                        {navigationRevision}
                        onMatchCount={handleMatchCount}
                        onMatchPositions={handleMatchPositions}
                        bind:scrollElement={codeScrollElement}
                    />
                </div>
                <OverviewRuler marks={rulerMarks} scrollElement={codeScrollElement} onMarkActivate={handleMarkActivate} />
            {:else if fileInfo.file_type === 'image' && fileInfo.uri}
                <ImageRenderer uri={fileInfo.uri} filename={fileInfo.filename} />
            {:else if fileInfo.file_type === 'video' && fileInfo.uri}
                <VideoRenderer uri={fileInfo.uri} />
            {:else}
                <div class="renderer-pane">
                    <TextRenderer
                        content={effectiveContent}
                        query={activeQuery}
                        {currentMatchIndex}
                        {navigationRevision}
                        onMatchCount={handleMatchCount}
                        onMatchPositions={handleMatchPositions}
                        bind:scrollElement={textScrollElement}
                    />
                </div>
                <OverviewRuler marks={rulerMarks} scrollElement={textScrollElement} onMarkActivate={handleMarkActivate} />
            {/if}
        </div>
    {:else}
        <div class="empty">
            <p>No file loaded</p>
        </div>
    {/if}
</div>

<style>
    :global(*) {
        box-sizing: border-box;
        margin: 0;
        padding: 0;
    }

    :global(html, body, #app) {
        height: 100%;
    }

    :global(body) {
        font-family: var(--font-sans, system-ui);
        color: var(--color-fg, #000);
        background: var(--color-bg, #fff);
        overflow: hidden;
    }

    .app-container {
        display: flex;
        flex-direction: column;
        height: 100%;
        width: 100%;
        background: var(--color-bg, #fff);
    }

    .loading,
    .error,
    .empty {
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        flex: 1;
        gap: 16px;
        padding: 32px;
        text-align: center;
    }

    .spinner {
        width: 32px;
        height: 32px;
        border: 3px solid var(--color-border, #ddd);
        border-top-color: var(--color-accent, #007bff);
        border-radius: 50%;
        animation: spin 1s linear infinite;
    }

    @keyframes spin {
        to {
            transform: rotate(360deg);
        }
    }

    .error {
        background: rgba(220, 38, 38, 0.05);
    }

    .error-title {
        font-size: 16px;
        font-weight: 600;
        color: var(--color-error, #dc2626);
    }

    .error-message {
        font-size: 14px;
        color: var(--color-fg-alt, #666);
        font-family: var(--font-mono, 'monospace');
        white-space: pre-wrap;
        max-width: 90%;
    }

    .content-area {
        flex: 1;
        min-height: 0;
        background: var(--color-bg, #fff);
        display: flex;
        flex-direction: row;
    }

    .renderer-pane {
        flex: 1;
        min-width: 0;
        height: 100%;
    }

    .markdown-layout {
        display: flex;
        height: 100%;
    }

    .markdown-content {
        flex: 1;
        overflow-y: auto;
        overflow-x: hidden;
        min-width: 0;
    }


</style>
