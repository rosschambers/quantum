<script lang="ts">
    /**
     * Bar button for killing windows. Left-click enters Hyprland's `hyprctl
     * kill` click-picker directly (crosshair; next clicked window is
     * force-killed). Right-click opens a menu offering killing the active
     * window, gracefully closing any open window by address, and the picker as
     * a menu entry. The raw picker can target a quantum-drawn surface (bar,
     * widgets, file explorer share one process), which would take the daemon
     * down; binding it to left-click is the owner's deliberate choice.
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

    /**
     * Left-click enters Hyprland's click-to-kill picker (`hyprctl kill`)
     * directly: the pointer becomes a crosshair and the next window clicked is
     * force-killed. This is the raw, unfiltered picker, so it can target any
     * surface including quantum's own (which would take the daemon down) — that
     * is the owner's deliberate choice for a fast one-click kill.
     */
    function pickWindowToKill(): void {
        runShell(['hyprctl', 'kill']);
    }

    // Right-click opens the kill menu (kill the active window, close a specific
    // open window, or enter the picker as a menu choice); left-click runs the
    // picker directly.
    $effect(() => {
        const node = buttonElement;
        if (!node) return;
        const teardownMenu = wireBarMenu(node, client, buildKillMenu, 'contextmenu');
        node.addEventListener('click', pickWindowToKill);
        return () => {
            node.removeEventListener('click', pickWindowToKill);
            teardownMenu();
        };
    });
</script>

<BarButton
    ariaLabel="Kill window"
    title="Open the window-kill menu"
    bindRef={(el) => (buttonElement = el)}
>
    <Icon name="pacman" size={18} />
</BarButton>
