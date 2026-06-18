<script lang="ts">
    import { untrack } from 'svelte';

    let now = $state(new Date());

    $effect(() => {
        const id = setInterval(() => { now = new Date(); }, 1000);
        return () => clearInterval(id);
    });

    let time = $derived(
        now.toLocaleTimeString(undefined, {
            hour: '2-digit',
            minute: '2-digit',
            second: '2-digit',
            hour12: true,
        }),
    );

    // The localized date string only changes when the calendar day changes, so
    // derive it from a day-granularity key. This derived depends solely on
    // dayKey, so toLocaleDateString runs once per day instead of every second.
    let dayKey = $derived(now.toDateString());

    let date = $derived.by(() => {
        dayKey;
        return untrack(() => now).toLocaleDateString(undefined, {
            weekday: 'short',
            month: 'short',
            day: 'numeric',
            year: 'numeric',
        });
    });
</script>

<div class="bar-clock" title={date + ' ' + time}>
    <span class="time">{time}</span>
</div>

<style>
    .bar-clock {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        font-variant-numeric: tabular-nums;
        font-size: 14px;
        font-weight: 600;
        color: var(--color-fg, #cdd6f4);
        line-height: 1;
        min-width: 64px;
        padding: 0 6px;
        cursor: default;
    }
</style>
