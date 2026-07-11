<script lang="ts">
    /**
     * Renders the StatusNotifierItem system tray at the start of the bar's
     * centre region: one SystemTrayButton per item. An empty tray renders
     * nothing at all so it reserves no width in the bar.
     *
     * This component owns only the provider subscription. Each button owns its
     * own menu/click wiring (see SystemTrayButton), keyed by `item.service`, so
     * an update to one item does not disturb the wiring of another. The
     * `{#each ... (item.service)}` key means Svelte only mounts or unmounts a
     * button when its service enters or leaves; an item update re-renders its
     * existing child with fresh props without remounting.
     */
    import type { Client, SystemTrayState } from '@quantum/client';
    import { SYSTEM_TRAY_CHANNEL, SYSTEM_TRAY_PROVIDER } from './channels';
    import SystemTrayButton from './SystemTrayButton.svelte';

    interface Props {
        client: Client;
    }

    let { client }: Props = $props();
    let state: SystemTrayState = $state({ items: [] });

    $effect(() => {
        client
            .call('provider.query', { id: SYSTEM_TRAY_PROVIDER })
            .then((result: unknown) => {
                if (result) state = result as SystemTrayState;
            })
            .catch(() => {});
        const unsubscribe = client.subscribe(SYSTEM_TRAY_CHANNEL, (payload: unknown) => {
            state = payload as SystemTrayState;
        });
        return () => unsubscribe?.();
    });
</script>

{#if state.items.length > 0}
    <div class="system-tray">
        {#each state.items as item (item.service)}
            <SystemTrayButton {client} {item} />
        {/each}
    </div>
{/if}

<style>
    .system-tray {
        display: flex;
        align-items: center;
        gap: 6px;
    }
</style>
