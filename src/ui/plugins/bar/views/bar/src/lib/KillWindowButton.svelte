<script lang="ts">
    /**
     * Bar button that invokes `hyprctl kill`, putting Hyprland into a
     * "click any window to kill it" mode (xkill-equivalent). The button
     * fires a one-shot shell action through the `shell_command` provider;
     * Hyprland handles the rest of the picker interaction.
     */
    import type { Client } from '@quantum/client';
    import Icon from './Icon.svelte';
    import BarButton from './BarButton.svelte';
    import { wireBarMenu } from './tray/barMenu';

    interface Props {
        client: Client;
    }

    let { client }: Props = $props();
    let buttonElement: HTMLButtonElement | undefined = $state(undefined);

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

    // Right-click: kill the focused window directly, or enter the picker.
    $effect(() => {
        const node = buttonElement;
        if (!node) return;
        return wireBarMenu(node, client, () => [
            {
                label: 'Kill active window',
                danger: true,
                onSelect: () => runShell(['hyprctl', 'dispatch', 'killactive']),
            },
            { label: 'Pick window to kill', onSelect: () => runShell(['hyprctl', 'kill']) },
        ]);
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
