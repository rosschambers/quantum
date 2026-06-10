<script lang="ts">
    import type { Client } from '@quantum/client';
    import type { MprisState } from '../lib/types';
    import Icon from './Icon.svelte';
    import BarButtonCluster from './BarButtonCluster.svelte';
    import BarClusterButton from './BarClusterButton.svelte';
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

<BarButtonCluster disabled={isDisabled(state)}>
    <BarClusterButton
        ariaLabel="Previous track"
        onclick={() => invokeCommand('previous')}
        disabled={isDisabled(state)}
    >
        <Icon name="prev" size={18} />
    </BarClusterButton>
    <BarClusterButton
        ariaLabel={isPlaying(state) ? 'Pause' : 'Play'}
        onclick={() => invokeCommand('play-pause')}
        disabled={isDisabled(state)}
    >
        <Icon name={isPlaying(state) ? 'pause' : 'play'} size={18} />
    </BarClusterButton>
    <BarClusterButton
        ariaLabel="Next track"
        onclick={() => invokeCommand('next')}
        disabled={isDisabled(state)}
    >
        <Icon name="next" size={18} />
    </BarClusterButton>
</BarButtonCluster>
