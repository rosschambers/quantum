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

<span class="signal" aria-label={`Signal ${percent} percent`}>
    <span class="bars">
        {#each [1, 2, 3, 4] as bar (bar)}
            <i class:on={bar <= litBars}></i>
        {/each}
    </span>
    <span class="pct" class:pct-green={percent > 70} class:pct-orange={percent > 40 && percent <= 70} class:pct-red={percent <= 40}>{percent}%</span>
</span>

<style>
    .signal {
        display: inline-flex;
        flex-direction: column;
        align-items: center;
        gap: 2px;
    }
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
    .pct {
        font-size: 9px;
        font-weight: 600;
        line-height: 1;
    }
    .pct-green {
        color: #a6e3a1;
    }
    .pct-orange {
        color: #f9e2af;
    }
    .pct-red {
        color: #f38ba8;
    }
</style>
