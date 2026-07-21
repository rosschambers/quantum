<script lang="ts">
    import { untrack } from 'svelte';
    import type { Client } from '@quantum/client';
    import CalendarDropdown from './CalendarDropdown.svelte';
    import { barViewName } from './tray/barMenu';

    interface Props {
        // The bar's IPC client, used to expand and reset the bar input region
        // while the calendar dropdown is open so it receives pointer clicks.
        // Optional so the clock still renders (display-only) without a client.
        client?: Client;
    }

    let { client }: Props = $props();

    let now = $state(new Date());
    let open = $state(false);
    let clockEl: HTMLDivElement | undefined = $state(undefined);
    let dropdownEl: HTMLDivElement | undefined = $state(undefined);

    $effect(() => {
        const id = setInterval(() => {
            now = new Date();
        }, 1000);
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

    // The bar surface gates pointer input to its visible strip; a dropdown
    // hanging below it is outside that region and would not receive clicks.
    // Expanding the input region to cover the dropdown's bounding box (and
    // resetting it on close) mirrors wireBarMenu's onPlaced/onClose handling.
    function expandInputRegion(rect: DOMRect): void {
        client
            ?.call('view.set_input_region', {
                name: barViewName(),
                region: {
                    x: Math.round(rect.x),
                    y: Math.round(rect.y),
                    width: Math.round(rect.width),
                    height: Math.round(rect.height),
                },
            })
            .catch(console.error);
    }

    function resetInputRegion(): void {
        client
            ?.call('view.set_input_region', { name: barViewName(), region: null })
            .catch(console.error);
    }

    function openCalendar(): void {
        open = true;
    }

    function closeCalendar(): void {
        if (!open) return;
        open = false;
        resetInputRegion();
    }

    function toggleCalendar(): void {
        if (open) {
            closeCalendar();
        } else {
            openCalendar();
        }
    }

    function onClockKeydown(event: KeyboardEvent): void {
        if (event.key === 'Enter' || event.key === ' ') {
            event.preventDefault();
            toggleCalendar();
        }
    }

    // Once the dropdown is in the DOM, expand the bar input region to cover it
    // so its buttons and wheel are clickable, and register the dismissal
    // listeners. Cleanup resets the region and removes the listeners.
    $effect(() => {
        if (!open) return;
        const node = dropdownEl;
        if (!node) return;

        expandInputRegion(node.getBoundingClientRect());

        const onDocumentPointerDown = (event: MouseEvent): void => {
            const target = event.target as Node | null;
            if (target && (node.contains(target) || clockEl?.contains(target))) {
                return;
            }
            closeCalendar();
        };
        const onKeydown = (event: KeyboardEvent): void => {
            if (event.key === 'Escape') closeCalendar();
        };

        document.addEventListener('pointerdown', onDocumentPointerDown, true);
        document.addEventListener('keydown', onKeydown);
        return () => {
            document.removeEventListener('pointerdown', onDocumentPointerDown, true);
            document.removeEventListener('keydown', onKeydown);
        };
    });
</script>

<div class="bar-clock-anchor">
    <div
        bind:this={clockEl}
        class="bar-clock"
        class:open
        role="button"
        tabindex="0"
        aria-haspopup="dialog"
        aria-expanded={open}
        title={date + ' ' + time}
        onclick={toggleCalendar}
        onkeydown={onClockKeydown}
    >
        <span class="time">{time}</span>
    </div>

    {#if open}
        <div bind:this={dropdownEl} class="bar-clock-dropdown">
            <CalendarDropdown />
        </div>
    {/if}
</div>

<style>
    .bar-clock-anchor {
        position: relative;
        display: inline-flex;
    }

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
        border-radius: 7px;
        cursor: pointer;
        user-select: none;
    }
    .bar-clock:hover {
        background: var(--color-surface);
    }
    .bar-clock.open {
        background: var(--color-surface-hover);
    }

    .bar-clock-dropdown {
        position: absolute;
        top: calc(100% + 6px);
        left: 50%;
        transform: translateX(-50%);
        z-index: 20;
    }
</style>
