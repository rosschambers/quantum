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

    let fileInfo: ViewerFileInfo | null = $state(null);
    let isLoading = $state(true);
    let error: string | null = $state(null);
    let path = $state('');

    const client = createClient();

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

        // Set up Escape key handler
        const handleKeyDown = (event: KeyboardEvent) => {
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
        <div class="content-area">
            {#if fileInfo.file_type === 'markdown'}
                <MarkdownRenderer content={fileInfo.content} />
            {:else if fileInfo.file_type === 'code' || fileInfo.file_type === 'json'}
                <CodeRenderer content={fileInfo.content} language={fileInfo.language} />
            {:else if fileInfo.file_type === 'image' && fileInfo.uri}
                <ImageRenderer uri={fileInfo.uri} filename={fileInfo.filename} />
            {:else if fileInfo.file_type === 'video' && fileInfo.uri}
                <VideoRenderer uri={fileInfo.uri} />
            {:else}
                <TextRenderer content={fileInfo.content} />
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
        overflow-y: auto;
        overflow-x: hidden;
        background: var(--color-bg, #fff);
    }


</style>
