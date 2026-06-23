<script lang="ts">
    import type { Client, MenuItem } from '@quantum/client';
    import type { AudioState } from '../types';
    import { AUDIO_CHANNEL, AUDIO_PROVIDER } from '../channels';
    import { gradientColor } from '../gradient';
    import { volumeIcon } from '../icons';
    import { onClick, onScroll } from './interaction';
    import { wireBarMenu } from './barMenu';
    import BarButton from '../BarButton.svelte';
    import Ring from '../Ring.svelte';

    /** Preset output volume levels offered in the right-click quick menu. */
    const MENU_PERCENTS = [25, 50, 75, 100];

    interface Props {
        client: Client;
    }

    let { client }: Props = $props();
    let state: AudioState = $state({ available: false, default_sink: null });
    let root: HTMLButtonElement | undefined = $state(undefined);

    $effect(() => {
        client
            .call('provider.query', { id: AUDIO_PROVIDER })
            .then((r: unknown) => {
                if (r) state = r as AudioState;
            })
            .catch(() => {});
        const unsubscribe = client.subscribe(AUDIO_CHANNEL, (payload: unknown) => {
            state = payload as AudioState;
        });
        return () => unsubscribe?.();
    });

    $effect(() => {
        if (!root) return;
        const off1 = onClick(root, handleClick, 'left');
        const off2 = onScroll(root, handleScroll);
        return () => {
            off1();
            off2();
        };
    });

    // Right-click: mute, output-volume presets, and a microphone mute toggle.
    $effect(() => {
        const node = root;
        if (!node) return;
        return wireBarMenu(node, client, buildMenuItems);
    });

    function audioCommand(payload: Record<string, unknown>): void {
        client
            .call('action.invoke', {
                provider: 'audio',
                action: { kind: 'custom', data: { kind: 'audio', payload } },
            })
            .catch((error) => console.error('audio command failed:', error));
    }

    function buildMenuItems(): MenuItem[] {
        const sink = state.default_sink;
        if (!sink) return [];
        const closest = MENU_PERCENTS.reduce((best, pct) =>
            Math.abs(pct - sink.volume_percent) < Math.abs(best - sink.volume_percent) ? pct : best,
        );
        const items: MenuItem[] = [
            { label: sink.muted ? 'Unmute' : 'Mute', onSelect: () => audioCommand({ command: 'toggle_mute' }) },
            { separator: true },
        ];
        for (const pct of MENU_PERCENTS) {
            items.push({
                label: `${pct}%`,
                icon: pct === closest ? '\u2022' : undefined,
                onSelect: () => audioCommand({ command: 'set_volume', percent: pct }),
            });
        }
        // Microphone mute, shown with live state when a default source exists.
        const source = state.default_source;
        if (source) {
            items.push({ separator: true });
            items.push({
                label: source.muted ? 'Unmute microphone' : 'Mute microphone',
                onSelect: () => audioCommand({ command: 'toggle_mic_mute' }),
            });
        }
        return items;
    }

    async function handleClick(): Promise<void> {
        if (!state.available || !state.default_sink) return;
        try {
            await client.call('action.invoke', {
                provider: 'audio',
                action: {
                    kind: 'custom',
                    data: { kind: 'audio', payload: { command: 'toggle_mute' } },
                },
            });
        } catch (err) {
            console.error('audio toggle_mute failed:', err);
        }
    }

    async function handleScroll(delta: 1 | -1): Promise<void> {
        if (!state.available || !state.default_sink) return;
        const volumeDelta = delta === 1 ? 5 : -5;
        try {
            await client.call('action.invoke', {
                provider: 'audio',
                action: {
                    kind: 'custom',
                    data: {
                        kind: 'audio',
                        payload: { command: 'adjust_volume', delta: volumeDelta },
                    },
                },
            });
        } catch (err) {
            console.error('audio adjust_volume failed:', err);
        }
    }

    function tooltipFor(s: AudioState): string {
        if (!s.default_sink) return 'audio unavailable';
        const muted = s.default_sink.muted ? ' (muted)' : '';
        return `${s.default_sink.description} \u00b7 ${s.default_sink.volume_percent}%${muted}`;
    }
</script>

{#if state.available && state.default_sink !== null}
    <BarButton title={tooltipFor(state)} bindRef={(el) => (root = el)}>
        <Ring
            percent={state.default_sink.muted ? 0 : state.default_sink.volume_percent}
            color={gradientColor(state.default_sink.volume_percent)}
            kind="icon"
            iconName={volumeIcon(state.default_sink.volume_percent, state.default_sink.muted)}
            size={18}
        />
    </BarButton>
{/if}
