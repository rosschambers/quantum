<script lang="ts">
    interface Props {
        /** Whether the expander tray is currently visible. */
        open: boolean;
        /**
         * Called when the user picks a preset OR submits a custom
         * delay. `delaySecs` is the resolved delay in seconds. The
         * parent runs `action.schedule` with this delay then closes
         * the expander.
         */
        onSchedule: (delaySecs: number) => Promise<void> | void;
        /**
         * Called when the user clicks the "Now" pill. The parent's
         * "Now" path is the immediate-action two-click confirm flow
         * — NOT a scheduled action. The expander just signals "the
         * user wants the immediate path"; the parent handles it.
         */
        onNow: () => void;
        /** Called when the expander should close (after Schedule/Now/cancel). */
        onClose: () => void;
    }

    let { open, onSchedule, onNow, onClose }: Props = $props();

    /** Whether the custom-input UI is showing instead of the preset pills. */
    let customMode = $state(false);
    /** Custom minutes input, clamped to 1-1440 on submit. */
    let customMinutes = $state(15);

    interface Preset {
        label: string;
        secs: number;
    }
    const PRESETS: Preset[] = [
        { label: '5m', secs: 5 * 60 },
        { label: '15m', secs: 15 * 60 },
        { label: '30m', secs: 30 * 60 },
        { label: '1h', secs: 60 * 60 },
    ];

    async function pickPreset(secs: number): Promise<void> {
        await onSchedule(secs);
        onClose();
    }

    function pickNow(): void {
        onNow();
        onClose();
    }

    async function submitCustom(): Promise<void> {
        const minutes = Math.max(1, Math.min(1440, Math.round(customMinutes)));
        await onSchedule(minutes * 60);
        customMode = false;
        customMinutes = 15;
        onClose();
    }

    function showCustom(): void {
        customMode = true;
    }

    function cancelCustom(): void {
        customMode = false;
        customMinutes = 15;
    }
</script>

{#if open}
    <div class="schedule-expander" role="group" aria-label="Schedule action">
        {#if customMode}
            <div class="custom-row">
                <input
                    type="number"
                    min="1"
                    max="1440"
                    step="1"
                    bind:value={customMinutes}
                    class="custom-input"
                    aria-label="Delay in minutes"
                />
                <span class="custom-suffix">min</span>
                <button type="button" class="pill primary" onclick={submitCustom}>Schedule</button>
                <button type="button" class="pill" onclick={cancelCustom}>Cancel</button>
            </div>
        {:else}
            <div class="preset-row">
                <button type="button" class="pill" onclick={pickNow}>Now</button>
                {#each PRESETS as preset}
                    <button
                        type="button"
                        class="pill"
                        data-preset={preset.label}
                        onclick={() => pickPreset(preset.secs)}
                    >{preset.label}</button>
                {/each}
                <button type="button" class="pill" onclick={showCustom}>Custom…</button>
            </div>
        {/if}
    </div>
{/if}

<style>
    .schedule-expander {
        padding: 8px 12px 12px 42px;
        margin: -2px 0 4px 0;
    }
    .preset-row,
    .custom-row {
        display: flex;
        gap: 6px;
        align-items: center;
        flex-wrap: wrap;
    }
    .pill {
        background: var(--color-bg, #1e1e2e);
        color: var(--color-fg, #cdd6f4);
        border: 1px solid var(--color-border, #45475a);
        border-radius: 999px;
        padding: 4px 12px;
        font-size: 12px;
        cursor: pointer;
        line-height: 1.4;
        transition: background-color 0.1s, border-color 0.1s;
    }
    .pill:hover {
        background: var(--color-bg-alt, #313244);
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
    .custom-input {
        width: 64px;
        background: var(--color-bg, #1e1e2e);
        color: var(--color-fg, #cdd6f4);
        border: 1px solid var(--color-border, #45475a);
        border-radius: 4px;
        padding: 4px 6px;
        font-size: 12px;
        font-family: var(--font-mono, ui-monospace, monospace);
    }
    .custom-suffix {
        font-size: 12px;
        color: var(--color-fg-alt, #a6adc8);
        margin-right: 8px;
    }
</style>
