<script lang="ts">
    /**
     * Renders the StatusNotifierItem system tray at the start of the bar's
     * centre region: one BarButton per item, each wired to the application's
     * own context menu. An empty tray renders nothing at all so it reserves no
     * width in the bar.
     *
     * Left-click activates the item (or, for menu-only items, opens the same
     * menu a right-click would). Right-click asks the application to refresh
     * its menu (`about_to_show`), re-queries the tray for the freshest menu
     * tree, and opens it through the shared `wireBarMenu` path.
     */
    import type { Client, MenuItem, SystemTrayItem, SystemTrayState } from '@quantum/client';
    import { SYSTEM_TRAY_CHANNEL, SYSTEM_TRAY_PROVIDER } from './channels';
    import { resolveIcon } from './icon';
    import { menuNodesToItems } from './systemTrayMenu';
    import { onClick } from './tray/interaction';
    import { wireBarMenu } from './tray/barMenu';
    import BarButton from './BarButton.svelte';

    interface Props {
        client: Client;
    }

    let { client }: Props = $props();
    let state: SystemTrayState = $state({ items: [] });
    let buttonRefs: Array<HTMLButtonElement | undefined> = $state([]);

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

    // Wire each item's button: its right-click application menu and its
    // left-click behaviour. Re-runs whenever the item set or the bound button
    // elements change, tearing down the previous listeners first.
    $effect(() => {
        const items = state.items;
        const cleanups: Array<() => void> = [];
        items.forEach((item, index) => {
            const node = buttonRefs[index];
            if (!node) return;
            cleanups.push(wireBarMenu(node, client, () => buildItemsFor(item)));
            cleanups.push(onClick(node, () => handleLeftClick(item, node), 'left'));
        });
        return () => {
            for (const off of cleanups) off();
        };
    });

    function trayCommand(payload: Record<string, unknown>): Promise<unknown> {
        return client.call('action.invoke', {
            provider: 'system_tray',
            action: { kind: 'custom', data: { kind: 'system_tray', payload } },
        });
    }

    // Ask the application to refresh its menu before it is shown. Non-fatal:
    // many items do not implement AboutToShow, so a failure is swallowed.
    async function aboutToShow(item: SystemTrayItem): Promise<void> {
        try {
            await trayCommand({ command: 'about_to_show', service: item.service });
        } catch (error) {
            console.error('system tray about_to_show failed:', error);
        }
    }

    async function buildItemsFor(item: SystemTrayItem): Promise<MenuItem[]> {
        await aboutToShow(item);
        let fresh: SystemTrayState = state;
        try {
            fresh = (await client.call('provider.query', {
                id: SYSTEM_TRAY_PROVIDER,
            })) as SystemTrayState;
        } catch (error) {
            console.error('system tray query failed:', error);
            fresh = state;
        }
        const match = fresh.items.find((candidate) => candidate.service === item.service) ?? item;
        return menuNodesToItems(match.service, match.menu, (itemId) =>
            trayCommand({
                command: 'menu_select',
                service: match.service,
                item_id: itemId,
            }).catch(console.error),
        );
    }

    function activate(item: SystemTrayItem): void {
        trayCommand({ command: 'activate', service: item.service }).catch(console.error);
    }

    function handleLeftClick(item: SystemTrayItem, node: HTMLButtonElement): void {
        // A menu-only item has no distinct activate action; left-click opens
        // the same menu a right-click would, routed through the identical
        // `wireBarMenu` handler by re-dispatching a contextmenu event.
        if (item.item_is_menu === true) {
            const rect = node.getBoundingClientRect();
            node.dispatchEvent(
                new MouseEvent('contextmenu', {
                    bubbles: true,
                    clientX: rect.left,
                    clientY: rect.bottom,
                }),
            );
            return;
        }
        activate(item);
    }
</script>

{#if state.items.length > 0}
    <div class="system-tray">
        {#each state.items as item, index (item.service)}
            {@const iconUrl = resolveIcon(item.icon)}
            <BarButton title={item.title} bindRef={(el) => (buttonRefs[index] = el)}>
                {#if iconUrl}
                    <img class="tray-icon" src={iconUrl} alt={item.title} />
                {:else}
                    <span class="tray-fallback">{item.title.charAt(0).toUpperCase()}</span>
                {/if}
            </BarButton>
        {/each}
    </div>
{/if}

<style>
    .system-tray {
        display: flex;
        align-items: center;
        gap: 6px;
    }
    .tray-icon {
        width: 18px;
        height: 18px;
        object-fit: contain;
        display: block;
    }
    .tray-fallback {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 18px;
        height: 18px;
        font-size: 13px;
        line-height: 1;
    }
</style>
