<script lang="ts">
    import type { Client } from '@quantum/client';
    import Icon from '../Icon.svelte';
    import BarButton from '../BarButton.svelte';

    interface Props {
        client: Client;
    }

    let { client }: Props = $props();

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
    </span>
</BarButton>

<style>
    .timer-icon {
        position: relative;
        display: inline-flex;
        align-items: center;
        justify-content: center;
    }
</style>
