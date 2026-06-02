<script lang="ts">
    /**
     * Bottom-of-modal delay picker. The selected delay applies to the
     * next action the user invokes. `value = 0` means "fire now"
     * (parent routes through `action.invoke`); any positive value is a
     * delay in seconds (parent routes through `action.schedule`).
     *
     * Implementation: a custom button + popover instead of the native
     * `<select>` because WebKit/GTK render the native dropdown list
     * with system colors that don't blend with the modal. The popover
     * is positioned absolutely above the trigger so it floats over the
     * scheduled-list section.
     */

    interface Props {
        /** Current delay in seconds; 0 means immediate. */
        value: number;
        /** Called whenever the resolved delay changes (preset pick or
         *  custom-input confirm). */
        onChange: (delaySecs: number) => void;
    }

    let { value, onChange }: Props = $props();

    interface Preset {
        label: string;
        secs: number;
    }
    const PRESETS: Preset[] = [
        { label: 'Now', secs: 0 },
        { label: '5 min', secs: 5 * 60 },
        { label: '15 min', secs: 15 * 60 },
        { label: '30 min', secs: 30 * 60 },
        { label: '1 hour', secs: 60 * 60 },
    ];

    /** Popover open state. */
    let open = $state(false);
    /** When `true` the custom-minutes input row is showing inside the
     *  popover instead of the preset list. */
    let customMode = $state(false);
    /** Custom value in minutes, clamped on confirm. */
    let customMinutes = $state(15);

    /**
     * Pretty label for the current value: the matching preset label
     * if one exists, otherwise "<n> min" / "<h>h <m>m" derived from
     * the value.
     */
    let triggerLabel = $derived.by(() => {
        const match = PRESETS.find((preset) => preset.secs === value);
        if (match) return match.label;
        const totalMinutes = Math.round(value / 60);
        const hours = Math.floor(totalMinutes / 60);
        const minutes = totalMinutes % 60;
        if (hours === 0) return `${minutes} min`;
        if (minutes === 0) return hours === 1 ? '1 hour' : `${hours} hours`;
        return `${hours}h ${minutes}m`;
    });

    function toggleOpen(): void {
        open = !open;
        if (!open) customMode = false;
    }

    function selectPreset(secs: number): void {
        onChange(secs);
        open = false;
        customMode = false;
    }

    function enterCustomMode(): void {
        customMode = true;
    }

    function confirmCustom(): void {
        const minutes = Math.max(1, Math.min(1440, Math.round(customMinutes)));
        onChange(minutes * 60);
        open = false;
        customMode = false;
    }

    function cancelCustom(): void {
        customMode = false;
    }

    /**
     * Close the popover when the user clicks anywhere outside the
     * picker. Bound to the document while open so we don't waste a
     * listener slot when the popover isn't showing.
     */
    let rootElement: HTMLDivElement;
    function handleDocumentClick(event: MouseEvent): void {
        if (!open) return;
        const target = event.target as Node | null;
        if (target && rootElement && !rootElement.contains(target)) {
            open = false;
            customMode = false;
        }
    }

    $effect(() => {
        if (!open) return;
        document.addEventListener('mousedown', handleDocumentClick);
        return () => document.removeEventListener('mousedown', handleDocumentClick);
    });
</script>

<!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
<div class="delay-picker" role="group" aria-label="Action delay" bind:this={rootElement}>
    <span class="picker-label">Delay</span>
    <button
        type="button"
        class="trigger"
        class:open
        aria-haspopup="listbox"
        aria-expanded={open}
        onclick={toggleOpen}
    >
        <span class="trigger-label">{triggerLabel}</span>
        <svg
            class="chevron"
            width="10"
            height="10"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2.5"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
        >
            <polyline points="6,9 12,15 18,9" />
        </svg>
    </button>

    {#if open}
        <div class="popover" role="listbox" aria-label="Delay presets">
            {#if customMode}
                <div class="custom-row">
                    <input
                        type="number"
                        min="1"
                        max="1440"
                        step="1"
                        bind:value={customMinutes}
                        class="custom-input"
                        aria-label="Custom delay in minutes"
                    />
                    <span class="custom-suffix">min</span>
                    <button type="button" class="pill primary" onclick={confirmCustom}>OK</button>
                    <button type="button" class="pill" onclick={cancelCustom}>Cancel</button>
                </div>
            {:else}
                {#each PRESETS as preset (preset.secs)}
                    <button
                        type="button"
                        class="option"
                        class:selected={preset.secs === value}
                        role="option"
                        aria-selected={preset.secs === value}
                        data-preset={preset.secs}
                        onclick={() => selectPreset(preset.secs)}
                    >{preset.label}</button>
                {/each}
                <button
                    type="button"
                    class="option custom-trigger"
                    data-preset="custom"
                    onclick={enterCustomMode}
                >Custom…</button>
            {/if}
        </div>
    {/if}
</div>

<style>
    .delay-picker {
        position: relative;
        display: flex;
        align-items: center;
        gap: 10px;
        padding: 10px 12px;
        margin-top: 12px;
        background: rgba(0, 0, 0, 0.2);
        border-radius: 8px;
        font-size: 12px;
    }
    .picker-label {
        color: var(--color-fg-alt, #a6adc8);
        text-transform: uppercase;
        letter-spacing: 0.08em;
        font-size: 10px;
        font-weight: 600;
    }
    .trigger {
        flex: 1;
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 8px;
        background: var(--color-bg, #1e1e2e);
        color: var(--color-fg, #cdd6f4);
        border: 1px solid var(--color-border, #45475a);
        border-radius: 6px;
        padding: 5px 10px;
        font-size: 12px;
        font-family: inherit;
        cursor: pointer;
        transition: border-color 0.1s;
    }
    .trigger:hover,
    .trigger.open {
        border-color: var(--color-accent, #89b4fa);
    }
    .trigger:focus {
        outline: none;
        border-color: var(--color-accent, #89b4fa);
    }
    .chevron {
        color: var(--color-fg-alt, #a6adc8);
        transition: transform 0.12s;
        flex-shrink: 0;
    }
    .trigger.open .chevron {
        transform: rotate(180deg);
    }
    .popover {
        position: absolute;
        bottom: calc(100% + 6px);
        right: 12px;
        left: 12px;
        background: var(--color-bg, #1e1e2e);
        border: 1px solid var(--color-border, #45475a);
        border-radius: 8px;
        box-shadow: 0 8px 24px rgba(0, 0, 0, 0.55);
        padding: 4px;
        display: flex;
        flex-direction: column;
        gap: 2px;
        z-index: 10;
    }
    .option {
        display: block;
        width: 100%;
        text-align: left;
        background: transparent;
        color: var(--color-fg, #cdd6f4);
        border: 1px solid transparent;
        border-radius: 5px;
        padding: 6px 10px;
        font-size: 12px;
        font-family: inherit;
        cursor: pointer;
        transition: background-color 0.08s, border-color 0.08s;
    }
    .option:hover {
        background: var(--color-bg-alt, #313244);
    }
    .option.selected {
        background: rgba(137, 180, 250, 0.15);
        color: var(--color-accent, #89b4fa);
        font-weight: 600;
    }
    .option.custom-trigger {
        color: var(--color-fg-alt, #a6adc8);
        font-style: italic;
        margin-top: 2px;
        border-top: 1px solid var(--color-border, #45475a);
        border-radius: 0 0 5px 5px;
        padding-top: 8px;
    }
    .custom-row {
        display: flex;
        align-items: center;
        gap: 6px;
        padding: 6px 4px;
        flex-wrap: wrap;
    }
    .custom-input {
        width: 64px;
        background: var(--color-bg-alt, #313244);
        color: var(--color-fg, #cdd6f4);
        border: 1px solid var(--color-border, #45475a);
        border-radius: 4px;
        padding: 4px 6px;
        font-size: 12px;
        font-family: var(--font-mono, ui-monospace, monospace);
    }
    .custom-suffix {
        color: var(--color-fg-alt, #a6adc8);
    }
    .pill {
        background: var(--color-bg-alt, #313244);
        color: var(--color-fg, #cdd6f4);
        border: 1px solid var(--color-border, #45475a);
        border-radius: 999px;
        padding: 4px 12px;
        font-size: 11px;
        cursor: pointer;
        line-height: 1.4;
        transition: background-color 0.1s, border-color 0.1s;
    }
    .pill:hover {
        border-color: var(--color-accent, #89b4fa);
    }
    .pill.primary {
        background: var(--color-accent, #89b4fa);
        color: var(--color-bg, #1e1e2e);
        border-color: var(--color-accent, #89b4fa);
        font-weight: 600;
    }
    .pill.primary:hover {
        opacity: 0.9;
    }
</style>
