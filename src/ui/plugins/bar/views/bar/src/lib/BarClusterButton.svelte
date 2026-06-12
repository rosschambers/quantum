<script lang="ts">
    /**
     * Individual button inside a BarButtonCluster. No own background;
     * the cluster owns the pill. Hover lightens the button's portion
     * of the cluster.
     */
    import type { Snippet } from 'svelte';

    interface Props {
        disabled?: boolean;
        title?: string;
        ariaLabel?: string;
        onclick?: (e: MouseEvent) => void | Promise<void>;
        children: Snippet;
    }

    let {
        disabled = false,
        title,
        ariaLabel,
        onclick,
        children,
    }: Props = $props();
</script>

<button
    type="button"
    class="cluster-button"
    {disabled}
    aria-label={ariaLabel}
    {title}
    {onclick}
>
    {@render children()}
</button>

<style>
    .cluster-button {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        background: transparent;
        border: none;
        padding: 4px 10px;
        color: var(--color-fg, #cdd6f4);
        font-family: inherit;
        font-size: inherit;
        line-height: 1;
        cursor: pointer;
        transition: background-color 0.13s ease, color 0.13s ease;
    }
    .cluster-button:hover:not(:disabled),
    .cluster-button:focus-visible {
        background: hsla(230, 14%, 42%, 1);
        outline: none;
    }

</style>
