<script lang="ts">
    import { createClient } from '@quantum/client';
    import type { ViewerFileInfo } from './types';

    interface Props {
        fileInfo: ViewerFileInfo;
    }

    let { fileInfo }: Props = $props();

    const client = createClient();

    function getFileTypeIcon(fileType: string): string {
        const icons: Record<string, string> = {
            markdown: 'M',
            json: '{}',
            code: '#',
            text: 'T',
            image: '*',
            video: '>',
        };
        return icons[fileType] || 'F';
    }

    async function closePanel() {
        try {
            await client.call('view.hide', { name: 'plugin/file-viewer/file-viewer' });
        } catch (error) {
            console.error('Failed to close panel:', error);
        }
    }
</script>

<header>
    <div class="header-left">
        <span class="icon">{getFileTypeIcon(fileInfo.file_type)}</span>
        <span class="filename">{fileInfo.filename}</span>
    </div>
    <div class="breadcrumb">
        <span class="directory">{fileInfo.directory}</span>
    </div>
    <div class="spacer"></div>
    <button class="icon-button" title="Close" onclick={closePanel}>&#x2715;</button>
</header>

<style>
    header {
        display: flex;
        align-items: center;
        gap: 12px;
        padding: 12px 16px;
        border-bottom: 1px solid var(--color-border, #ddd);
        background: var(--color-bg);
        height: 44px;
        flex-shrink: 0;
    }

    .header-left {
        display: flex;
        align-items: center;
        gap: 8px;
        min-width: 0;
    }

    .icon {
        flex-shrink: 0;
        font-size: 11px;
        font-family: var(--font-mono, monospace);
        font-weight: 700;
        color: var(--color-accent, #a6e3a1);
        background: color-mix(in oklab, var(--color-accent, #a6e3a1) 12%, transparent);
        padding: 2px 5px;
        border-radius: 3px;
        line-height: 1;
    }

    .filename {
        font-family: var(--font-mono, 'monospace');
        font-size: 13px;
        font-weight: 500;
        color: var(--color-fg, #000);
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }

    .breadcrumb {
        display: flex;
        align-items: center;
        min-width: 0;
        flex: 1;
    }

    .directory {
        font-size: 12px;
        color: var(--color-fg-alt, #666);
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }

    .spacer {
        flex: 1;
    }

    .icon-button {
        background: transparent;
        border: none;
        color: var(--color-fg, #000);
        cursor: pointer;
        padding: 6px 8px;
        font-size: 16px;
        display: flex;
        align-items: center;
        justify-content: center;
        border-radius: 4px;
        transition: background-color 200ms;
        flex-shrink: 0;
    }

    .icon-button:hover {
        background: var(--color-surface, #eee);
    }

    .icon-button:active {
        background: var(--color-border, #ddd);
    }
</style>
