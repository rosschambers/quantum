<script lang="ts">
    import { slugify } from './types';

    interface TocEntry {
        level: number;
        text: string;
        id: string;
    }

    interface Props {
        content: string;
        contentElement: HTMLElement | undefined;
    }

    let { content, contentElement }: Props = $props();

    let activeId = $state('');
    let collapsed = $state(false);

    let headings: TocEntry[] = $derived.by(() => {
        const entries: TocEntry[] = [];
        const regex = /^(#{1,6})\s+(.+)$/gm;
        let match: RegExpExecArray | null;
        while ((match = regex.exec(content)) !== null) {
            entries.push({
                level: match[1].length,
                text: match[2].trim(),
                id: slugify(match[2].trim()),
            });
        }
        return entries;
    });

    $effect(() => {
        const element = contentElement;
        if (!element) return;

        const headingElements = element.querySelectorAll('h1[id], h2[id], h3[id], h4[id], h5[id], h6[id]');
        if (headingElements.length === 0) return;

        const observer = new IntersectionObserver(
            (entries) => {
                for (const entry of entries) {
                    if (entry.isIntersecting) {
                        activeId = entry.target.id;
                    }
                }
            },
            {
                root: element.closest('.content-area'),
                rootMargin: '0px 0px -80% 0px',
                threshold: 0,
            }
        );

        for (const heading of headingElements) {
            observer.observe(heading);
        }

        return () => observer.disconnect();
    });

    function scrollToHeading(id: string) {
        if (!contentElement) return;
        const target = contentElement.querySelector(`#${CSS.escape(id)}`);
        if (target) {
            target.scrollIntoView({ behavior: 'smooth' });
        }
    }
</script>

<nav class="toc-sidebar" class:collapsed>
    <button class="toc-toggle" onclick={() => collapsed = !collapsed} title={collapsed ? 'Show contents' : 'Hide contents'}>
        {collapsed ? '\u25B6' : '\u25C0'}
    </button>
    {#if !collapsed}
        <div class="toc-title">Contents</div>
        <ul class="toc-list">
            {#each headings as heading}
                <li
                    class="toc-entry"
                    class:active={activeId === heading.id}
                    style="padding-left: {(heading.level - 1) * 12}px"
                >
                    <button onclick={() => scrollToHeading(heading.id)}>
                        {heading.text}
                    </button>
                </li>
            {/each}
        </ul>
    {/if}
</nav>

<style>
    .toc-sidebar {
        width: 200px;
        background: var(--color-bg);
        border-right: 1px solid var(--color-border);
        flex-shrink: 0;
        display: flex;
        flex-direction: column;
        overflow: hidden;
        position: relative;
        transition: width 150ms ease;
    }

    .toc-sidebar.collapsed {
        width: 28px;
    }

    .toc-toggle {
        position: absolute;
        top: 8px;
        right: 4px;
        width: 20px;
        height: 20px;
        border: none;
        background: none;
        color: var(--color-muted);
        font-size: 10px;
        cursor: pointer;
        display: flex;
        align-items: center;
        justify-content: center;
        border-radius: 3px;
        z-index: 1;
    }

    .toc-toggle:hover {
        color: var(--color-fg);
        background: var(--color-surface-hover);
    }

    .toc-title {
        padding: 16px 12px;
        font-size: 13px;
        font-weight: 600;
        color: var(--color-fg, #000);
        border-bottom: 1px solid var(--color-border, #ddd);
        flex-shrink: 0;
    }

    .toc-list {
        list-style: none;
        padding: 0;
        margin: 0;
        overflow-y: auto;
        flex: 1;
    }

    .toc-entry {
        margin: 0;
        padding: 0;
    }

    .toc-entry button {
        width: 100%;
        padding: 8px 12px;
        border: none;
        background: none;
        font-family: inherit;
        font-size: 13px;
        text-align: left;
        cursor: pointer;
        color: var(--color-fg-alt, #666);
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }

    .toc-entry button:hover {
        background: var(--color-surface, #f0f0f0);
    }

    .toc-entry.active button {
        color: var(--color-accent, #007bff);
        font-weight: 600;
    }
</style>
