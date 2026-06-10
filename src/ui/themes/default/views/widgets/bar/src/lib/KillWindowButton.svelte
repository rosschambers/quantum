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

    interface Props {
        client: Client;
    }

    let { client }: Props = $props();

    async function invokeKill(): Promise<void> {
        try {
            await client.call('action.invoke', {
                provider: 'shell_command',
                action: {
                    kind: 'shell',
                    data: { command: ['hyprctl', 'kill'], terminal: false },
                },
            });
        } catch (err) {
            console.error('hyprctl kill failed:', err);
        }
    }
</script>

<BarButton
    ariaLabel="Kill window"
    title="Click then pick a window to kill"
    onclick={invokeKill}
>
    <Icon name="target" size={14} />
</BarButton>
