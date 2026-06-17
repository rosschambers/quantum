<script lang="ts">
    import { createTimerStore, type Client } from '@quantum/client';
    import Icon from '../Icon.svelte';
    import BarButton from '../BarButton.svelte';

    interface Props {
        client: Client;
    }

    let { client }: Props = $props();
    let count = $state(0);

    $effect(() => {
        const off = createTimerStore(client).subscribe((data) => {
            count = data.timers.filter((t) => t.status === 'active').length;
        });
        return () => off?.();
    });

    function badgeLabel(n: number): string {
        return n > 9 ? '9+' : String(n);
    }

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
</script>

<BarButton ariaLabel="Timers" onclick={openCreate}>
    <span class="timer-icon">
        <Icon name="timer" size={18} />
        {#if count > 0}
            <span class="timer-badge">{badgeLabel(count)}</span>
        {/if}
    </span>
</BarButton>

<style>
    .timer-icon {
        position: relative;
        display: inline-flex;
        align-items: center;
        justify-content: center;
    }
    .timer-badge {
        position: absolute;
        top: -6px;
        right: -8px;
        min-width: 14px;
        height: 14px;
        padding: 0 3px;
        box-sizing: border-box;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        border-radius: 7px;
        background: var(--color-accent, #f38ba8);
        color: var(--color-bg, #1e1e2e);
        font-size: 10px;
        line-height: 1;
        font-weight: 600;
    }
</style>
