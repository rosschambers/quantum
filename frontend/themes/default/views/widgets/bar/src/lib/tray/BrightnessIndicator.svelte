<script lang="ts">
    import type { Client } from '@quantum/client';
    import type { BrightnessState } from '../types';
    import { BRIGHTNESS_CHANNEL } from '../channels';
    import { gradientColor } from '../gradient';
    import { Icons } from '../icons';
    import { onScroll } from './interaction';
    import Ring from '../Ring.svelte';

    interface Props {
        client: Client;
    }

    let { client }: Props = $props();
    let state: BrightnessState = $state({ available: false, displays: [] });
    let root: HTMLElement | undefined = $state(undefined);

    $effect(() => {
        client
            .call('provider.query', { id: 'brightness' })
            .then((r: unknown) => {
                if (r) state = r as BrightnessState;
            })
            .catch(() => {});
        const unsubscribe = client.subscribe(BRIGHTNESS_CHANNEL, (payload: unknown) => {
            state = payload as BrightnessState;
        });
        return () => unsubscribe?.();
    });

    $effect(() => {
        if (!root) return;
        const off = onScroll(root, handleScroll);
        return () => off();
    });

    async function handleScroll(delta: 1 | -1): Promise<void> {
        if (!state.available || state.displays.length === 0) return;
        const display = state.displays[0];
        const deltaPercent = delta === 1 ? 5 : -5;
        try {
            await client.call('action.invoke', {
                provider: 'brightness',
                action: {
                    kind: 'custom',
                    data: {
                        kind: 'brightness',
                        payload: {
                            command: 'adjust',
                            subsystem: display.subsystem,
                            name: display.name,
                            delta_percent: deltaPercent,
                        },
                    },
                },
            });
        } catch (err) {
            console.error('brightness adjust failed:', err);
        }
    }

    function brightnessPercent(s: BrightnessState): number | null {
        if (s.displays.length === 0) return null;
        const d = s.displays[0];
        if (d.max === 0) return null;
        return (d.current / d.max) * 100;
    }

    function tooltipFor(s: BrightnessState): string {
        if (s.displays.length === 0) return 'brightness unavailable';
        const d = s.displays[0];
        const pct = brightnessPercent(s);
        if (pct === null) return `${d.subsystem}/${d.name}`;
        return `${d.subsystem}/${d.name} \u00b7 ${Math.round(pct)}%`;
    }
</script>

{#if state.available && state.displays.length > 0}
    <div bind:this={root} class="tray-icon brightness" title={tooltipFor(state)}>
        <span class="icon" aria-hidden="true">{Icons.brightness}</span>
        <Ring
            percent={brightnessPercent(state)}
            color={gradientColor(brightnessPercent(state))}
        />
    </div>
{/if}

<style>
    .tray-icon {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        font-size: var(--tray-icon-size, 14px);
        color: var(--tray-icon-color, var(--color-fg, #cdd6f4));
        padding: 0 4px;
        user-select: none;
        cursor: pointer;
        line-height: 1;
    }
    .icon {
        font-family:
            'JetBrainsMono Nerd Font',
            'Symbols Nerd Font',
            'FontAwesome',
            var(--font-mono, ui-monospace, monospace);
        font-size: var(--tray-icon-size, 14px);
        line-height: 1;
    }
</style>
