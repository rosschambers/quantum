<script lang="ts">
    import { createTimerStore, type Client, type Timer } from '@quantum/client';
    import Icon from '../Icon.svelte';
    import BarButton from '../BarButton.svelte';
    import { wireBarMenu } from './barMenu';
    import { ringTarget, remainingSeconds, RingTotals } from './soonest';

    interface Props {
        client: Client;
    }

    let { client }: Props = $props();
    let buttonEl: HTMLButtonElement | undefined = $state(undefined);

    let timers = $state<Timer[]>([]);
    let nowUnix = $state(Math.floor(Date.now() / 1000));

    // Drain denominators persist across renders, so the RingTotals instance
    // must live at script level, not inside an effect.
    const totals = new RingTotals();
    // Plain (not reactive) bookkeeping of the ids seen in the last snapshot, so
    // ids that leave the active set can be forgotten from the drain totals.
    let knownIds = new Set<string>();

    const SIZE = 18;
    const R = 6.4;
    const CIRC = 2 * Math.PI * R;

    // Fired-wins target: an expired timer still in the store takes priority over
    // a still-counting active one, so a fired timer is never missed. `target` is
    // null only when there are no timers at all.
    const target = $derived(ringTarget(timers, nowUnix));
    const remaining = $derived(target ? remainingSeconds(target.timer, nowUnix) : 0);
    const fired = $derived(target?.fired ?? false);
    const dashoffset = $derived(
        CIRC * (1 - (target ? totals.fraction(target.timer.id, remaining) : 0)),
    );

    $effect(() => {
        const off = createTimerStore(client).subscribe((data) => {
            timers = data.timers;
            const nextIds = new Set(data.timers.map((t) => t.id));
            for (const id of knownIds) {
                if (!nextIds.has(id)) totals.forget(id);
            }
            knownIds = nextIds;
        });
        return () => off?.();
    });

    // Tick the clock once a second so the derived remaining time and ring
    // fraction advance while the bar is mounted.
    $effect(() => {
        const handle = setInterval(() => {
            nowUnix = Math.floor(Date.now() / 1000);
        }, 1000);
        return () => clearInterval(handle);
    });

    // Right-click opens the quick-actions menu via the shared bar-menu helper,
    // which handles the bar's input-region passthrough and dropdown anchoring.
    $effect(() => {
        const node = buttonEl;
        if (!node) return;
        return wireBarMenu(node, client, () => [
            { label: 'Open timers', onSelect: openCreate },
            { label: 'Dismiss all', onSelect: dismissAll },
        ]);
    });

    async function openCreate(): Promise<void> {
        // The bar widget is injected with a per-monitor `__quantum_monitor`
        // global (e.g. "DP-1") by the WebView host. Append it as an
        // `@monitor` suffix so the registry pins the timer-create overlay to
        // the same monitor as the bar that triggered it.
        const monitor = window.__quantum_monitor;
        const name = monitor
            ? `plugin/timer-create/timer-create@${monitor}`
            : 'plugin/timer-create/timer-create';
        try {
            await client.call('view.toggle', { name });
        } catch (err) {
            console.error(`view.toggle ${name} failed:`, err);
        }
    }

    async function dismissAll(): Promise<void> {
        try {
            await client.call('timer.dismiss_all', {});
        } catch (err) {
            console.error('timer.dismiss_all failed:', err);
        }
    }
</script>

<div class="timer-root">
    <BarButton ariaLabel="Timers" onclick={openCreate} bindRef={(el) => (buttonEl = el)}>
        <span class="icon-box" style={`width:${SIZE}px;height:${SIZE}px`}>
            {#if target === null}
                <Icon name="timer" size={SIZE} />
            {:else}
                <svg
                    data-testid="timer-ring"
                    data-fired={fired}
                    class="ring {fired ? 'fired' : ''}"
                    width={SIZE}
                    height={SIZE}
                    viewBox="0 0 16 16"
                    aria-hidden="true"
                >
                    <circle class="track" cx="8" cy="8" r={R} />
                    <circle
                        class="prog"
                        cx="8"
                        cy="8"
                        r={R}
                        stroke-dasharray={CIRC}
                        stroke-dashoffset={dashoffset}
                    />
                </svg>
            {/if}
        </span>
    </BarButton>
</div>

<style>
    .timer-root {
        position: relative;
        display: inline-flex;
    }
    .icon-box {
        position: relative;
        display: inline-flex;
        align-items: center;
        justify-content: center;
    }
    .ring {
        transform: rotate(-90deg);
    }
    .ring circle {
        fill: none;
    }
    .ring .track {
        stroke: color-mix(in srgb, var(--color-fg-alt, #9fb0a2) 26%, transparent);
        stroke-width: 2.4;
    }
    .ring .prog {
        stroke: var(--color-accent, #8fc7a0);
        stroke-width: 2.4;
        stroke-linecap: round;
        transition: stroke-dashoffset 0.3s linear;
    }
    .ring.fired .prog,
    .ring.fired .track {
        stroke: var(--color-error, #e07a6a);
    }
    .ring.fired {
        animation: timer-pulse 1.05s ease-in-out infinite;
    }
    @keyframes timer-pulse {
        0% {
            box-shadow: 0 0 0 0 color-mix(in srgb, var(--color-error, #e07a6a) 55%, transparent);
        }
        50% {
            box-shadow: 0 0 0 6px color-mix(in srgb, var(--color-error, #e07a6a) 0%, transparent);
        }
        100% {
            box-shadow: 0 0 0 0 color-mix(in srgb, var(--color-error, #e07a6a) 0%, transparent);
        }
    }
</style>
