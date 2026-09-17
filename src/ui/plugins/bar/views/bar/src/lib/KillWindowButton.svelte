<script lang="ts">
    /**
     * Bar button for killing windows. Left-click enters the click-picker:
     * `qkill` when installed (the quantum-aware picker — resolves the clicked
     * point against Hyprland's window list, gracefully closes quantum's own
     * xdg-toplevel windows through `closewindow`, and force-kills everything
     * else), falling back to plain `hyprctl kill` (crosshair; next clicked
     * window is force-killed) when it is not. Right-click opens a menu
     * offering killing the active window, gracefully closing any open window
     * by address, and the picker as a menu entry. The menu closes by address
     * (a graceful close request), which quantum handles per-window — a
     * quantum window no longer needs to be excluded. Killing a quantum LAYER
     * surface (bar, clock, timers) still takes the whole daemon down: layer
     * surfaces cannot receive a close request, and every quantum surface shares
     * one process. `qkill` preserves kill semantics for those.
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
    // (each closes by Hyprland address), or enter the picker. When the
    // window list is empty or the query fails, only the two static items show.
    // Every entry in the Hyprland window list is an xdg-toplevel that can take
    // a graceful close request — quantum's own windows included (closing one
    // closes just that window; the daemon routes the close per-window). Layer
    // surfaces never appear in this list and stay the picker's job.
    async function buildKillMenu(): Promise<MenuItem[]> {
        const windows = await fetchWindows();
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
     * Left-click enters the click-picker. Prefers `qkill` — the quantum-aware
     * picker shipped alongside the daemon (it resolves the clicked point,
     * gracefully closes quantum's own xdg-toplevel windows, and force-kills
     * everything else). Falls back to plain `hyprctl kill` when `qkill` is not
     * on PATH. The `if` form matters: a bare `qkill || hyprctl kill` would
     * fire the fallback even when the user cancelled the picker with Escape.
     */
    function pickWindowToKill(): void {
        runShell([
            'sh',
            '-c',
            'if command -v qkill >/dev/null 2>&1; then exec qkill; else exec hyprctl kill; fi',
        ]);
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
