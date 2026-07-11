<script lang="ts">
    /**
     * Bar button that opens a window-kill menu on left-click. The menu offers
     * killing the active window and gracefully closing any open window by
     * address, plus a deliberate "Pick window to kill" entry that enters
     * Hyprland's `hyprctl kill` click-picker. Left-click no longer runs the
     * picker directly: force-killing a quantum-drawn surface would take down
     * the whole daemon (bar, widgets, file explorer are one process), so the
     * raw picker is only reachable as a conscious menu choice.
     */
    import type { Client, MenuItem } from '@quantum/client';
    import Icon from './Icon.svelte';
    import BarButton from './BarButton.svelte';
    import { wireBarMenu } from './tray/barMenu';
    import type { WindowList, WindowListEntry } from './types';

    interface Props {
        client: Client;
    }

    let { client }: Props = $props();
    let buttonElement: HTMLButtonElement | undefined = $state(undefined);

    /** Longest window title shown in the menu before it is ellipsized. */
    const MAXIMUM_TITLE_LENGTH = 40;

    /**
     * The GtkApplication identifier of the quantum daemon itself. Every
     * quantum-drawn surface (bar, widgets, file explorer) reports this class,
     * so windows carrying it are excluded from the kill list: they share the
     * daemon process, and closing one would take the whole session down.
     */
    const QUANTUM_APPLICATION_ID = 'dev.quantum.daemon';

    function runShell(command: string[]): void {
        client
            .call('action.invoke', {
                provider: 'shell',
                action: { kind: 'shell', data: { command, terminal: false } },
            })
            .catch((error) => console.error(`${command.join(' ')} failed:`, error));
    }

    function truncateTitle(title: string): string {
        if (title.length <= MAXIMUM_TITLE_LENGTH) return title;
        return `${title.slice(0, MAXIMUM_TITLE_LENGTH - 1)}\u2026`;
    }

    /** Query the live window list, degrading to an empty list on any error. */
    async function fetchWindows(): Promise<WindowListEntry[]> {
        try {
            const result = (await client.call('provider.query', {
                id: 'hyprland-windows',
            })) as WindowList | undefined;
            return result?.windows ?? [];
        } catch (error) {
            console.error('window list query failed:', error);
            return [];
        }
    }

    // Build the kill menu: kill the active window, the list of open windows
    // (each closes by Hyprland address), or enter the click-picker. When the
    // window list is empty or the query fails, only the two static items show.
    // Quantum's own windows are filtered out so the daemon can never be
    // selected as a target.
    async function buildKillMenu(): Promise<MenuItem[]> {
        const windows = (await fetchWindows()).filter(
            (entry) => entry.class !== QUANTUM_APPLICATION_ID,
        );
        const items: MenuItem[] = [
            {
                label: 'Kill active window',
                danger: true,
                onSelect: () => runShell(['hyprctl', 'dispatch', 'killactive']),
            },
        ];

        if (windows.length > 0) {
            items.push({ separator: true, label: '' });
            for (const entry of windows) {
                const windowClass = entry.class;
                items.push({
                    label: `${windowClass} \u2014 ${truncateTitle(entry.title)}`,
                    onSelect: () =>
                        runShell([
                            'hyprctl',
                            'dispatch',
                            'closewindow',
                            `address:${entry.address}`,
                        ]),
                });
            }
        }

        items.push({ separator: true, label: '' });
        items.push({
            label: 'Pick window to kill',
            onSelect: () => runShell(['hyprctl', 'kill']),
        });

        return items;
    }

    // Left-click: open the kill menu (list open windows to close, kill the
    // active window, or deliberately enter the picker). Wiring the menu to the
    // 'click' trigger replaces the old force-kill left-click action.
    $effect(() => {
        const node = buttonElement;
        if (!node) return;
        return wireBarMenu(node, client, buildKillMenu, 'click');
    });
</script>

<BarButton
    ariaLabel="Kill window"
    title="Open the window-kill menu"
    bindRef={(el) => (buttonElement = el)}
>
    <Icon name="pacman" size={18} />
</BarButton>
