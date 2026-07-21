<script lang="ts">
    import { monthGrid, isToday, monthLabel } from './calendar';

    interface Props {
        // The reference "now" the calendar opens on and the Today button jumps
        // back to. Defaults to the real clock; tests pass a fixed date to keep
        // rendering deterministic.
        initialDate?: Date;
    }

    let { initialDate = new Date() }: Props = $props();

    // `now` is the fixed reference for today-highlighting. The displayed month
    // is tracked separately as a year plus zero-based month so prev/next/wheel
    // can move through months without mutating `now`.
    const now = initialDate;
    let viewYear = $state(initialDate.getFullYear());
    let viewMonth0 = $state(initialDate.getMonth());

    const weekdayHeaders = ['Su', 'Mo', 'Tu', 'We', 'Th', 'Fr', 'Sa'];

    let label = $derived(monthLabel(viewYear, viewMonth0));
    let cells = $derived(monthGrid(viewYear, viewMonth0));

    let fullDate = $derived(
        now.toLocaleDateString(undefined, {
            weekday: 'long',
            month: 'long',
            day: 'numeric',
            year: 'numeric',
        }),
    );

    function shiftMonth(delta: number): void {
        // Normalize through a Date so a December-to-January or reverse rollover
        // carries the year correctly.
        const next = new Date(viewYear, viewMonth0 + delta, 1);
        viewYear = next.getFullYear();
        viewMonth0 = next.getMonth();
    }

    function goToday(): void {
        viewYear = now.getFullYear();
        viewMonth0 = now.getMonth();
    }

    function onWheel(event: WheelEvent): void {
        event.preventDefault();
        shiftMonth(event.deltaY > 0 ? 1 : -1);
    }
</script>

<div class="calendar" role="dialog" aria-label="Calendar">
    <div class="head">
        <div class="month">{label}</div>
        <div class="nav">
            <button
                class="prev"
                type="button"
                title="Previous month"
                aria-label="Previous month"
                onclick={() => shiftMonth(-1)}>&lsaquo;</button
            >
            <button class="today-btn" type="button" onclick={goToday}>Today</button>
            <button
                class="next"
                type="button"
                title="Next month"
                aria-label="Next month"
                onclick={() => shiftMonth(1)}>&rsaquo;</button
            >
        </div>
    </div>

    <div class="grid" role="grid" onwheel={onWheel}>
        {#each weekdayHeaders as weekday (weekday)}
            <div class="dow">{weekday}</div>
        {/each}
        {#each cells as cell (`${viewYear}-${viewMonth0}-${cell.inMonth}-${cell.day}`)}
            <div
                class="day"
                class:other={!cell.inMonth}
                class:today={isToday(cell, viewYear, viewMonth0, now)}
            >
                {cell.day}
            </div>
        {/each}
    </div>

    <div class="foot">
        <span>{fullDate}</span>
        <span>Scroll to change month</span>
    </div>
</div>

<style>
    .calendar {
        width: 300px;
        background: var(--color-bg-alt);
        border: 1px solid var(--color-border);
        border-radius: 12px;
        box-shadow: 0 14px 40px var(--color-shadow);
        padding: 14px;
        font-family: var(--font-sans);
    }

    .head {
        display: flex;
        align-items: center;
        justify-content: space-between;
        margin-bottom: 10px;
    }
    .month {
        font-size: 15px;
        font-weight: 600;
        color: var(--color-fg);
    }
    .nav {
        display: flex;
        gap: 4px;
    }
    .nav button,
    .today-btn {
        background: transparent;
        border: 1px solid var(--color-border);
        color: var(--color-fg-alt);
        border-radius: 6px;
        cursor: pointer;
        width: 26px;
        height: 26px;
        font-size: 14px;
        line-height: 1;
        display: flex;
        align-items: center;
        justify-content: center;
    }
    .today-btn {
        width: auto;
        padding: 0 10px;
        font-size: 12px;
    }
    .nav button:hover,
    .today-btn:hover {
        background: var(--color-surface-hover);
    }

    .grid {
        display: grid;
        grid-template-columns: repeat(7, 1fr);
        gap: 2px;
    }
    .dow {
        text-align: center;
        font-size: 11px;
        color: var(--color-muted);
        padding: 4px 0;
        text-transform: uppercase;
        letter-spacing: 0.04em;
    }
    .day {
        text-align: center;
        font-size: 13px;
        color: var(--color-fg-alt);
        padding: 7px 0;
        border-radius: 7px;
    }
    .day.other {
        color: var(--color-border);
    }
    .day.today {
        background: var(--color-accent);
        color: var(--color-bg);
        font-weight: 700;
    }

    .foot {
        margin-top: 10px;
        padding-top: 10px;
        border-top: 1px solid var(--color-border);
        display: flex;
        justify-content: space-between;
        align-items: center;
        font-size: 12px;
        color: var(--color-muted);
    }
</style>
