<script lang="ts">
    import type { Client, MenuItem } from '@quantum/client';
    import type { BrightnessDisplay, BrightnessState } from '../types';
    import { BRIGHTNESS_CHANNEL, BRIGHTNESS_PROVIDER } from '../channels';
    import { gradientColor } from '../gradient';
    import { wireBarMenu } from './barMenu';

    import { onClick, onScroll } from './interaction';

    /** Preset brightness levels offered in the right-click quick menu. */
    const MENU_PERCENTS = [1, 25, 50, 75, 100];

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

    // Right-click offers preset brightness levels for the primary backlight.
    $effect(() => {
        const node = root;
        if (!node) return;
        return wireBarMenu(node, client, buildMenuItems);
    });

    function primaryDisplay(): BrightnessDisplay | undefined {
        const displays = state.displays;
        if (displays.length === 0) return undefined;
        return displays.find((d) => d.subsystem === 'backlight') ?? displays[0];
    }

    function buildMenuItems(): MenuItem[] {
        const display = primaryDisplay();
        if (!display || display.max === 0) return [];
        const currentPercent = Math.round((display.current / display.max) * 100);
        const closest = MENU_PERCENTS.reduce((best, pct) =>
            Math.abs(pct - currentPercent) < Math.abs(best - currentPercent) ? pct : best,
        );
        return MENU_PERCENTS.map((pct) => ({
            label: `${pct}%`,
            // Mark the preset closest to the current level with a leading dot.
            icon: pct === closest ? '\u2022' : undefined,
            onSelect: () =>
                client
                    .call('action.invoke', {
                        provider: 'brightness',
                        action: {
                            kind: 'custom',
                            data: {
                                kind: 'brightness',
                                payload: {
                                    command: 'set',
                                    subsystem: display.subsystem,
                                    name: display.name,
                                    value: Math.round((pct / 100) * display.max),
                                },
                            },
                        },
                    })
                    .catch((err) => console.error('brightness set failed:', err)),
        }));
    }

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
