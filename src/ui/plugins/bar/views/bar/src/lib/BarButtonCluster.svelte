<script lang="ts">
    /**
     * Pill-styled wrapper for a row of related buttons (e.g. the three
     * media-control buttons: previous, play/pause, next). The cluster
     * owns the background pill; individual buttons inside it should
     * NOT have their own background.
     *
     * Pair with `BarClusterButton` for the inner buttons so they get
     * the right hover treatment and dividers.
     */
    import type { Snippet } from 'svelte';

    interface Props {
        /** When true, the cluster is greyed out (e.g. media controls
         *  with no player). */
        disabled?: boolean;
        children: Snippet;
    }

    let { disabled = false, children }: Props = $props();
</script>

<div class="bar-button-cluster" class:disabled>
    {@render children()}
</div>

<style>
    .bar-button-cluster {
        display: inline-flex;
        align-items: stretch;
        background: var(--color-surface, hsla(230, 14%, 22%, 0.8));
        border: 1px solid rgba(255, 255, 255, 0.06);
        border-radius: 10px;
        overflow: hidden;
        transition: background-color 0.13s ease, border-color 0.13s ease;
    }
    .bar-button-cluster.disabled {
        opacity: 0.4;
    }
    /* Dividers between successive buttons inside the cluster. Owned
       here instead of on the buttons so we can flip them off in one
       place if we ever drop dividers. */
    .bar-button-cluster :global(.cluster-button + .cluster-button) {
        border-left: 1px solid var(--color-divider, rgba(255, 255, 255, 0.32));
    }
</style>
