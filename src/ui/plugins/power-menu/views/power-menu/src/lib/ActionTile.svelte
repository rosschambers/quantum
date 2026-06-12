<script lang="ts">
    import Icon, { type IconName } from './Icon.svelte';
    import type { PowerCommand } from '../lib/types';

    interface Props {
        command: PowerCommand;
        label: string;
        icon: IconName;
        /** `'primary'` renders the larger top-tier tile (shutdown,
         *  restart); `'secondary'` renders the smaller bottom-tier
         *  pill (suspend, hibernate, lock). */
        variant: 'primary' | 'secondary';
        /** When `armed` matches this tile's command, render as armed
         *  (red tint, "Confirm <Label>" text). Lifted to the parent so
         *  only one tile arms at a time. */
        armed: PowerCommand | null;
        /** Called on first click; parent updates `armed` and starts the
         *  disarm timer. */
        onArm: (command: PowerCommand) => void;
        /** Called when the user confirms (second click within window).
         *  Parent decides whether to invoke immediately or schedule
         *  based on the current delay-picker value. */
        onInvoke: () => Promise<void> | void;
    }

    let { command, label, icon, variant, armed, onArm, onInvoke }: Props = $props();

    let isArmed = $derived(armed === command);
    let iconSize = $derived(variant === 'primary' ? 26 : 20);

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
    class="action-tile {variant}"
    class:armed={isArmed}
    onclick={handleClick}
    data-command={command}
>
    <Icon name={icon} size={iconSize} />
    <span class="label">{isArmed ? `Confirm ${label}` : label}</span>
</button>

<style>
    .action-tile {
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        gap: 8px;
        background: rgba(0, 0, 0, 0.15);
        color: var(--color-fg, #cdd6f4);
        border: 1px solid var(--color-border, #45475a);
        border-radius: 10px;
        cursor: pointer;
        text-align: center;
        transition: background-color 0.1s, border-color 0.1s, color 0.1s;
    }
    .action-tile.primary {
        padding: 18px 14px;
        font-size: 13px;
        font-weight: 600;
    }
    .action-tile.secondary {
        padding: 10px 8px;
        font-size: 11px;
        font-weight: 500;
        gap: 6px;
        background: transparent;
        border-color: transparent;
    }
    .action-tile:hover:not(.armed) {
        background: var(--color-bg, #1e1e2e);
        border-color: var(--color-accent, #89b4fa);
    }
    .action-tile.secondary:hover:not(.armed) {
        border-color: var(--color-border, #45475a);
    }
    .action-tile.armed {
        background: rgba(243, 139, 168, 0.15);
        border-color: var(--color-error, #f38ba8);
        color: var(--color-error, #f38ba8);
    }
    .label {
        line-height: 1.2;
    }
</style>
