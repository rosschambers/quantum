<script lang="ts">
    import { onMount } from 'svelte';
    import { createClient } from '@quantum/client';
    import type { ViewerFileInfo } from './lib/types';
    import Header from './lib/Header.svelte';
    import MarkdownRenderer from './lib/MarkdownRenderer.svelte';
    import CodeRenderer from './lib/CodeRenderer.svelte';
    import ImageRenderer from './lib/ImageRenderer.svelte';
    import VideoRenderer from './lib/VideoRenderer.svelte';
    import TextRenderer from './lib/TextRenderer.svelte';
    import TocSidebar from './lib/TocSidebar.svelte';
    import FormatBanner from './lib/FormatBanner.svelte';
    import JsonFoldRenderer from './lib/JsonFoldRenderer.svelte';

    let fileInfo: ViewerFileInfo | null = $state(null);
    let isLoading = $state(true);
    let error: string | null = $state(null);
    let path = $state('');
    let markdownContentElement: HTMLElement | undefined = $state(undefined);
    let displayContent: string | null = $state(null);

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

    onMount(async () => {
        const args = (window as any).__quantum_args;
        if (!args?.path) {
            error = 'No file path provided';
            isLoading = false;
            return;
        }

        path = args.path;

        try {
            const result = await client.call('file-viewer.read', { path });
            fileInfo = result as ViewerFileInfo;
            isLoading = false;
        } catch (err) {
            const errorMessage = err instanceof Error ? err.message : String(err);
            error = `Failed to read file: ${errorMessage}`;
            isLoading = false;
        }

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

            if (event.key === 'Escape') {
                client.call('view.hide', { name: 'plugin/file-viewer/file-viewer' }).catch(console.error);
            }
        };

        window.addEventListener('keydown', handleKeyDown);
        return () => {
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
        <div class="content-area">
            {#if fileInfo.file_type === 'markdown'}
                <div class="markdown-layout" class:has-toc={markdownHeadingCount >= 3}>
                    {#if markdownHeadingCount >= 3}
                        <TocSidebar content={effectiveContent} contentElement={markdownContentElement} />
                    {/if}
                    <div class="markdown-content" bind:this={markdownContentElement}>
                        <MarkdownRenderer content={effectiveContent} />
                    </div>
                </div>
            {:else if fileInfo.file_type === 'json'}
                <JsonFoldRenderer content={effectiveContent} />
            {:else if fileInfo.file_type === 'code'}
                <CodeRenderer content={effectiveContent} language={fileInfo.language} />
            {:else if fileInfo.file_type === 'image' && fileInfo.uri}
                <ImageRenderer uri={fileInfo.uri} filename={fileInfo.filename} />
            {:else if fileInfo.file_type === 'video' && fileInfo.uri}
                <VideoRenderer uri={fileInfo.uri} />
            {:else}
                <TextRenderer content={effectiveContent} />
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
