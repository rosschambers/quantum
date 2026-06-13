<script lang="ts">
    /**
     * Pill-styled interactive button for the bar's tray and media row.
     * Used directly for solo buttons (PowerMenuIndicator,
     * VolumeIndicator). For multi-button clusters (MediaControls) wrap
     * children in BarButtonCluster instead and use BarClusterButton for
     * individual buttons inside it.
     */
    import type { Snippet } from 'svelte';

    interface Props {
        /** Set to true to render as a non-interactive container (e.g.
         *  when the underlying component is currently disabled or shows
         *  only an icon without click semantics). */
        disabled?: boolean;
        title?: string;
        ariaLabel?: string;
        onclick?: (e: MouseEvent) => void | Promise<void>;
        bindRef?: (el: HTMLButtonElement) => void;
        children: Snippet;
    }

    let {
        disabled = false,
        title,
        ariaLabel,
        onclick,
        bindRef,
        children,
    }: Props = $props();

    let root: HTMLButtonElement | undefined = $state(undefined);
    $effect(() => {
        if (root && bindRef) bindRef(root);
    });
</script>

<button
    type="button"
    bind:this={root}
    class="bar-button"
    class:disabled
    {disabled}
    aria-label={ariaLabel}
    {title}
    {onclick}
>
    {@render children()}
</button>

<style>
    .bar-button {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        gap: 6px;
        background: var(--color-surface, hsla(230, 14%, 22%, 0.8));
        border: 1px solid rgba(255, 255, 255, 0.06);
        border-radius: 10px;
        padding: 4px 10px;
        color: var(--color-fg-alt, #a6adc8);
        font-family: inherit;
        font-size: inherit;
        line-height: 1;
        cursor: pointer;
        transition: background-color 0.13s ease, color 0.13s ease,
            border-color 0.13s ease;
    }
    .bar-button:hover:not(:disabled),
    .bar-button:focus-visible {
        background: var(--color-surface-hover, hsla(230, 14%, 42%, 1));
        color: var(--color-fg, #cdd6f4);
        outline: none;
    }
    .bar-button:disabled {
        opacity: 0.4;
    }
</style>
