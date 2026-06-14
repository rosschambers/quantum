<script lang="ts">
    /**
     * Signal strength rendered as four ascending bars. The number of
     * lit bars maps the 0-100 percentage onto quartiles, matching the
     * approved playground design.
     */
    interface Props {
        percent: number;
    }

    const { percent }: Props = $props();

    const litBars = $derived(
        percent > 75 ? 4 : percent > 50 ? 3 : percent > 25 ? 2 : 1,
    );
</script>

<span class="bars" aria-label={`Signal ${percent} percent`}>
    {#each [1, 2, 3, 4] as bar (bar)}
        <i class:on={bar <= litBars}></i>
    {/each}
</span>

<style>
    .bars {
        display: inline-flex;
        align-items: flex-end;
        gap: 2px;
        height: 14px;
    }
    .bars i {
        width: 3px;
        background: var(--color-border, #45475a);
        border-radius: 1px;
        display: block;
    }
    .bars i.on {
        background: var(--color-fg, #cdd6f4);
    }
    .bars i:nth-child(1) {
        height: 4px;
    }
    .bars i:nth-child(2) {
        height: 7px;
    }
    .bars i:nth-child(3) {
        height: 10px;
    }
    .bars i:nth-child(4) {
        height: 14px;
    }
</style>
