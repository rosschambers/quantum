<script lang="ts">
    import { onMount } from 'svelte';
    import type { Client } from '@quantum/client';
    import type { ActiveWindowState } from '../lib/types';

    interface Props {
        client: Client;
    }

    let { client }: Props = $props();
    let state: ActiveWindowState | null = $state(null);

    onMount(() => {
        const unsubscribe = client.subscribe('hyprland.activewindow.event', (payload: unknown) => {
            state = payload as ActiveWindowState;
        });
        return () => unsubscribe?.();
    });

    function displayTitle(s: ActiveWindowState | null): string {
        if (!s) return '';
        const text = s.title.trim() || s.class.trim();
        return text;
    }

    function displayWorkspace(s: ActiveWindowState | null): string | null {
        if (!s) return null;
        if (s.workspace_name.startsWith('special:')) return null;
        return s.workspace_name || `${s.workspace_id}`;
    }
</script>

<div class="active-window">
    {#if displayWorkspace(state)}
        <span class="workspace">{displayWorkspace(state)}</span>
    {/if}
    <span class="title">{displayTitle(state)}</span>
</div>

<style>
    .active-window {
        display: flex;
        align-items: center;
        gap: var(--space-2, 0.5rem);
        min-width: 0;
        overflow: hidden;
    }
    .workspace {
        font-size: var(--font-size-sm, 12px);
        background: var(--color-bg-alt, #313244);
        color: var(--color-fg-alt, #a6adc8);
        padding: 2px 8px;
        border-radius: var(--radius-sm, 2px);
    }
    .title {
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }
</style>
