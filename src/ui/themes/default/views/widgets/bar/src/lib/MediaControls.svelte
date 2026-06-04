<script lang="ts">
    import type { Client } from '@quantum/client';
    import type { MprisState } from '../lib/types';
    import Icon from './Icon.svelte';
    import { MPRIS_CHANNEL } from './channels';

    interface Props {
        client: Client;
    }

    let { client }: Props = $props();
    let state: MprisState | null = $state(null);

    // `$effect` runs reliably in testing-library + Svelte 5 where `onMount` does not.
    // Behaviour at runtime in WebKit is the same.
    $effect(() => {
        const unsubscribe = client.subscribe(MPRIS_CHANNEL, (payload: unknown) => {
            state = payload as MprisState;
        });
        return () => unsubscribe?.();
    });

    function isPlaying(s: MprisState | null): boolean {
        return s?.playback_status === 'playing';
    }

    function isDisabled(s: MprisState | null): boolean {
        return s === null || s.player_id === null;
    }

    async function invokeCommand(command: 'previous' | 'play-pause' | 'next'): Promise<void> {
        if (isDisabled(state)) return;
        try {
            await client.call('action.invoke', {
                provider: 'mpris',
                action: {
                    kind: 'custom',
                    data: {
                        kind: 'mpris',
                        payload: { command },
                    },
                },
            });
        } catch (err) {
            console.error(`mpris ${command} failed:`, err);
        }
    }
</script>

<div class="media-controls" class:disabled={isDisabled(state)}>
    <button type="button" aria-label="Previous track" onclick={() => invokeCommand('previous')} disabled={isDisabled(state)}>
        <Icon name="prev" size={14} />
    </button>
    <button type="button" aria-label={isPlaying(state) ? 'Pause' : 'Play'} onclick={() => invokeCommand('play-pause')} disabled={isDisabled(state)}>
        <Icon name={isPlaying(state) ? 'pause' : 'play'} size={14} />
    </button>
    <button type="button" aria-label="Next track" onclick={() => invokeCommand('next')} disabled={isDisabled(state)}>
        <Icon name="next" size={14} />
    </button>
</div>

<style>
    .media-controls {
        display: flex;
        gap: var(--space-1, 0.25rem);
    }
    .media-controls.disabled button {
        opacity: 0.4;
        cursor: not-allowed;
    }
    button {
        background: transparent;
        color: var(--color-fg, #cdd6f4);
        border: none;
        padding: 2px 8px;
        cursor: pointer;
        font-size: var(--font-size-base, 14px);
    }
    button:hover:not(:disabled) {
        background: var(--color-bg-alt, #313244);
        border-radius: var(--radius-sm, 2px);
    }
    button:disabled {
        cursor: not-allowed;
    }
</style>
