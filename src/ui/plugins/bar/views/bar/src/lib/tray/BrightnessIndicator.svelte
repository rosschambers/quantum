<script lang="ts">
    import type { Client } from '@quantum/client';
    import type { BrightnessState } from '../types';
    import { BRIGHTNESS_CHANNEL, BRIGHTNESS_PROVIDER } from '../channels';
    import { gradientColor } from '../gradient';

    import { onClick, onScroll } from './interaction';

    /** Click toggles between these two brightness percentages. The
     *  threshold (15%) splits them so the click does the opposite of
     *  whatever the current level is closer to. */
    const DIM_PERCENT = 1;
    const BRIGHT_PERCENT = 30;
    const TOGGLE_THRESHOLD = 15;
    import BarButton from '../BarButton.svelte';
    import Ring from '../Ring.svelte';

    interface Props {
        client: Client;
    }

    let { client }: Props = $props();
    let state: BrightnessState = $state({ available: false, displays: [] });
    let root: HTMLButtonElement | undefined = $state(undefined);

    $effect(() => {
        client
            .call('provider.query', { id: BRIGHTNESS_PROVIDER })
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
        const offScroll = onScroll(root, handleScroll);
        const offClick = onClick(root, handleClick, 'left');
        return () => {
            offScroll();
            offClick();
        };
    });

    async function handleClick(): Promise<void> {
        if (!state.available || state.displays.length === 0) return;
        const display = state.displays[0];
        if (display.max === 0) return;
        const currentPct = (display.current / display.max) * 100;
        // If brightness is above the threshold, dim it. Otherwise raise
        // it. Click alternates between DIM_PERCENT and BRIGHT_PERCENT.
        const targetPct = currentPct > TOGGLE_THRESHOLD ? DIM_PERCENT : BRIGHT_PERCENT;
        const targetValue = Math.max(0, Math.round((targetPct / 100) * display.max));
        try {
            await client.call('action.invoke', {
                provider: 'brightness',
                action: {
                    kind: 'custom',
                    data: {
                        kind: 'brightness',
                        payload: {
                            command: 'set',
                            subsystem: display.subsystem,
                            name: display.name,
                            value: targetValue,
                        },
                    },
                },
            });
        } catch (err) {
            console.error('brightness toggle failed:', err);
        }
    }

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
    <BarButton title={tooltipFor(state)} bindRef={(el) => (root = el)}>
        <Ring
            percent={brightnessPercent(state)}
            color={gradientColor(brightnessPercent(state))}
            kind="icon"
            iconName="sun"
            size={18}
        />
    </BarButton>
{/if}
