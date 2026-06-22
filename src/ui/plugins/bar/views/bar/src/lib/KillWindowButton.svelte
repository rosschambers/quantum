<script lang="ts">
    /**
     * Bar button that invokes `hyprctl kill`, putting Hyprland into a
     * "click any window to kill it" mode (xkill-equivalent). The button
     * fires a one-shot shell action through the `shell_command` provider;
     * Hyprland handles the rest of the picker interaction.
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
            .catch((err) => console.error(`${command.join(' ')} failed:`, err));
    }

    async function invokeKill(): Promise<void> {
        runShell(['hyprctl', 'kill']);
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
        } catch (err) {
            console.error('window list query failed:', err);
            return [];
        }
    }

    // Build the kill menu: kill the active window, the list of open windows
    // (each closes by Hyprland address), or enter the click-picker. When the
    // window list is empty or the query fails, only the two static items show.
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

    // Right-click: list open windows to close, or enter the picker.
    $effect(() => {
        const node = buttonElement;
        if (!node) return;
        return wireBarMenu(node, client, buildKillMenu);
    });
</script>

<BarButton
    ariaLabel="Kill window"
    title="Click then pick a window to kill"
    onclick={invokeKill}
    bindRef={(el) => (buttonElement = el)}
>
    <Icon name="pacman" size={18} />
</BarButton>
