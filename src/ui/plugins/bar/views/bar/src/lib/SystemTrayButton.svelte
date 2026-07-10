<script lang="ts">
    /**
     * A single system tray button: one BarButton wired to the application's own
     * context menu. Left-click activates the item (or, for menu-only items,
     * opens the same menu a right-click would). Right-click asks the application
     * to refresh its menu (`about_to_show`), re-queries the tray for the
     * freshest menu tree, and opens it through the shared `wireBarMenu` path.
     *
     * The menu/click wiring lives here, per item, so a broadcast that only
     * updates THIS item's fields (a NewTitle/NewToolTip/NewStatus, or a
     * LayoutUpdated on the very item being viewed) re-renders the button with
     * fresh props WITHOUT re-running the wiring effect. That matters because
     * `wireBarMenu`'s teardown closes the context menu unconditionally: a
     * wiring effect that depended on the item fields would tear down and slam
     * an open menu shut on every update. The wiring effect below depends only
     * on the stable button node, so state churn never disturbs an open menu;
     * the callbacks read the freshest item lazily via `latest` at click time.
     */
    import type { Client, MenuItem, SystemTrayItem, SystemTrayState } from '@quantum/client';
    import { SYSTEM_TRAY_PROVIDER } from './channels';
    import { resolveIcon } from './icon';
    import { menuNodesToItems } from './systemTrayMenu';
    import { onClick } from './tray/interaction';
    import { wireBarMenu } from './tray/barMenu';
    import BarButton from './BarButton.svelte';

    interface Props {
        client: Client;
        item: SystemTrayItem;
    }

    let { client, item }: Props = $props();

    let node: HTMLButtonElement | undefined = $state(undefined);
    let iconUrl = $derived(resolveIcon(item.icon));

    // Mirror the latest item prop into `latest` so the wiring effect's
    // callbacks, which are defined once and never re-created, always act on the
    // freshest item without the wiring effect itself depending on item fields.
    // The mirror effect runs on mount, before any click, so `latest` is always
    // set by the time a callback reads it.
    let latest!: SystemTrayItem;
    $effect(() => {
        latest = item;
    });

    // Wire this button's right-click application menu and its left-click
    // behaviour. Depends ONLY on the stable button node, so it wires once and
    // is not torn down when the item updates.
    $effect(() => {
        const buttonNode = node;
        if (!buttonNode) return;
        const offMenu = wireBarMenu(buttonNode, client, () => buildItemsFor(latest));
        const offClick = onClick(buttonNode, () => handleLeftClick(latest, buttonNode), 'left');
        return () => {
            offMenu();
            offClick();
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
    async function aboutToShow(target: SystemTrayItem): Promise<void> {
        try {
            await trayCommand({ command: 'about_to_show', service: target.service });
        } catch (error) {
            console.error('system tray about_to_show failed:', error);
        }
    }

    async function buildItemsFor(target: SystemTrayItem): Promise<MenuItem[]> {
        await aboutToShow(target);
        let fresh: SystemTrayState = { items: [target] };
        try {
            fresh = (await client.call('provider.query', {
                id: SYSTEM_TRAY_PROVIDER,
            })) as SystemTrayState;
        } catch (error) {
            console.error('system tray query failed:', error);
            fresh = { items: [target] };
        }
        const match = fresh.items.find((candidate) => candidate.service === target.service) ?? target;
        return menuNodesToItems(match.service, match.menu, (itemId) =>
            trayCommand({
                command: 'menu_select',
                service: match.service,
                item_id: itemId,
            }).catch(console.error),
        );
    }

    function activate(target: SystemTrayItem): void {
        trayCommand({ command: 'activate', service: target.service }).catch(console.error);
    }

    function handleLeftClick(target: SystemTrayItem, buttonNode: HTMLButtonElement): void {
        // A menu-only item has no distinct activate action; left-click opens
        // the same menu a right-click would, routed through the identical
        // `wireBarMenu` handler by re-dispatching a contextmenu event.
        if (target.item_is_menu === true) {
            const rect = buttonNode.getBoundingClientRect();
            buttonNode.dispatchEvent(
                new MouseEvent('contextmenu', {
                    bubbles: true,
                    clientX: rect.left,
                    clientY: rect.bottom,
                }),
            );
            return;
        }
        activate(target);
    }
</script>

<BarButton title={item.title} bindRef={(el) => (node = el)}>
    {#if iconUrl}
        <img class="tray-icon" src={iconUrl} alt={item.title} />
    {:else}
        <span class="tray-fallback">{item.title.charAt(0).toUpperCase()}</span>
    {/if}
</BarButton>

<style>
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
