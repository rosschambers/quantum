<script lang="ts">
    import Icon, { type IconName } from './Icon.svelte';
    import type { PowerCommand } from '../lib/types';

    interface Props {
        command: PowerCommand;
        label: string;
        icon: IconName;
        /** Called when the user confirms (second click within window). */
        onInvoke: () => Promise<void> | void;
        /** When `armed` matches this row's command, render as armed. Lifted
         *  to the parent so only one row arms at a time. */
        armed: PowerCommand | null;
        /** Called on first click; parent updates `armed` and starts the
         *  disarm timer. */
        onArm: (command: PowerCommand) => void;
    }

    let { command, label, icon, onInvoke, armed, onArm }: Props = $props();

    let isArmed = $derived(armed === command);

    async function handleClick(): Promise<void> {
        if (isArmed) {
            await onInvoke();
        } else {
            onArm(command);
        }
    }
</script>

<button
    type="button"
    class="action-row"
    class:armed={isArmed}
    onclick={handleClick}
    data-command={command}
>
    <span class="label">{isArmed ? `Confirm ${label}` : label}</span>
    <span class="icon-slot"><Icon name={icon} size={18} /></span>
</button>

<style>
    .action-row {
        display: flex;
        align-items: center;
        gap: 12px;
        width: 100%;
        padding: 10px 12px;
        background: transparent;
        color: var(--color-fg, #cdd6f4);
        border: 1px solid transparent;
        border-radius: 6px;
        cursor: pointer;
        font-size: 14px;
        text-align: left;
        margin: 2px 0;
        transition: background-color 0.1s, border-color 0.1s, color 0.1s;
    }
    .action-row:hover:not(.armed) {
        background: var(--color-bg, #1e1e2e);
    }
    .action-row.armed {
        background: rgba(243, 139, 168, 0.15);
        border-color: var(--color-error, #f38ba8);
        color: var(--color-error, #f38ba8);
        font-weight: 600;
    }
    .icon-slot {
        display: inline-flex;
        line-height: 0;
    }
    .label {
        flex: 1;
    }
</style>
