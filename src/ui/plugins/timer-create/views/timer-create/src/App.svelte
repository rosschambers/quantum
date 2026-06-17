<script lang="ts">
    import { createClient } from '@quantum/client';
    import type {
        Weekday,
        SoundName,
        NotifyConfig,
        VisualConfig,
        VisualStyle,
        TimerStoreData,
    } from '@quantum/client';
    import { parseDurationToSecs, parseTimeOfDay } from './lib/parse';
    import { ALL_WEEKDAYS, recurrenceDays, type Recurrence } from './lib/recurrence';
    import StylePicker from './lib/StylePicker.svelte';

    /** Bare canonical view name; the registry strips any `@<monitor>` suffix
     * for single-instance overlays, so `view.hide` uses the bare name. */
    const VIEW_NAME = 'plugin/timer-create/timer-create';

    /** Sensible hardcoded defaults used when the `timer.list` fetch fails or
     * returns nothing. The daemon also fills omitted fields from
     * `settings.defaults_*`, but a complete object is sent for determinism. */
    const FALLBACK_NOTIFY: NotifyConfig = {
        notification: true,
        sound: null,
        urgency_ramp: true,
        ramp_threshold: 0.1,
        pulse: false,
        flash: false,
    };
    const FALLBACK_VISUAL: VisualConfig = {
        style: 'mixed',
        size: 120,
        thickness: 8,
        fill: false,
        reverse: false,
        accent_hue: 210,
        track_opacity: 0.2,
        label_visibility: 'always',
        time_visibility: 'always',
        text_position: 'below',
        text_color: 'accent',
        time_format: 'clock',
        font_scale: 1,
        font_weight: 500,
        uppercase: false,
    };

    const SOUND_NAMES: SoundName[] = ['complete', 'bell', 'chime', 'alarm'];

    const client = createClient();

    let label: string = $state('');
    let mode: 'in' | 'at' = $state('in');
    let durationText: string = $state('');
    let timeText: string = $state('');
    let recurrence: Recurrence = $state('none');
    let customDays: Weekday[] = $state([]);
    let error: string | null = $state(null);

    // Complete config objects fetched from the daemon; form controls below
    // override individual fields, and the merged result is submitted.
    let defaultsNotify: NotifyConfig = $state({ ...FALLBACK_NOTIFY });
    let defaultsVisual: VisualConfig = $state({ ...FALLBACK_VISUAL });

    // Alerting form state.
    let notification: boolean = $state(true);
    let soundOn: boolean = $state(false);
    let soundName: SoundName = $state('complete');
    let urgencyRamp: boolean = $state(true);

    // Visual form state, fed from StylePicker.
    let visualStyle: VisualStyle = $state('mixed');
    let visualAccentHue: number = $state(210);

    function toggleDay(day: Weekday): void {
        customDays = customDays.includes(day)
            ? customDays.filter((existing) => existing !== day)
            : [...ALL_WEEKDAYS].filter(
                  (candidate) => candidate === day || customDays.includes(candidate),
              );
    }

    function applyDefaults(notify: NotifyConfig, visual: VisualConfig): void {
        defaultsNotify = notify;
        defaultsVisual = visual;
        notification = notify.notification;
        soundOn = notify.sound !== null;
        soundName = notify.sound ?? 'complete';
        urgencyRamp = notify.urgency_ramp;
        visualStyle = visual.style;
        visualAccentHue = visual.accent_hue;
    }

    function onStyleChange(partial: { style: VisualStyle; accent_hue: number }): void {
        visualStyle = partial.style;
        visualAccentHue = partial.accent_hue;
    }

    // Seed form defaults from the daemon's current timer settings. Reads no
    // reactive state, so this effect runs once.
    $effect(() => {
        client
            .call('timer.list', {})
            .then((result) => {
                const data = result as TimerStoreData | undefined;
                const settings = data?.settings;
                if (settings?.defaults_notify && settings?.defaults_visual) {
                    applyDefaults(settings.defaults_notify, settings.defaults_visual);
                }
            })
            .catch(() => {
                // Non-fatal; the hardcoded fallbacks already seed the form.
            });
    });

    $effect(() => {
        document.addEventListener('keydown', onKeyDown);
        return () => {
            document.removeEventListener('keydown', onKeyDown);
            client.close();
        };
    });

    function onKeyDown(event: KeyboardEvent): void {
        if (event.key === 'Escape') {
            event.preventDefault();
            close();
        }
    }

    function onBackdropClick(event: MouseEvent): void {
        if (event.target === event.currentTarget) close();
    }

    function close(): void {
        client.call('view.hide', { name: VIEW_NAME }).catch(() => {});
    }

    /** Build the `start` object for the current mode, or set an inline error
     * and return `null` when the inputs do not parse. */
    function buildStart(): Record<string, unknown> | null {
        if (mode === 'in') {
            const secs = parseDurationToSecs(durationText);
            if (secs === null) {
                error = 'Enter a duration like 45m, 2h, or 1h30m.';
                return null;
            }
            return { kind: 'duration', secs };
        }
        const time = parseTimeOfDay(timeText);
        if (time === null) {
            error = 'Enter a time like 17:15.';
            return null;
        }
        if (recurrence === 'none') {
            return { kind: 'at', time };
        }
        const days = recurrenceDays(recurrence, customDays);
        if (days === null) {
            error = 'Pick at least one day for a custom recurrence.';
            return null;
        }
        return { kind: 'recurring', days, time };
    }

    async function submit(): Promise<void> {
        error = null;
        const start = buildStart();
        if (start === null) return;

        // Merge the form values over the fetched defaults so a complete
        // NotifyConfig / VisualConfig is always sent.
        const notify: NotifyConfig = {
            ...defaultsNotify,
            notification,
            sound: soundOn ? soundName : null,
            urgency_ramp: urgencyRamp,
        };
        const visual: VisualConfig = {
            ...defaultsVisual,
            style: visualStyle,
            accent_hue: visualAccentHue,
        };

        try {
            await client.call('timer.create', { label, start, visual, notify });
        } catch (err) {
            console.error('timer.create failed:', err);
            error = 'Could not create the timer.';
            return;
        }
        close();
    }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
<div class="backdrop" onclick={onBackdropClick}>
    <div class="card" role="dialog" aria-label="New timer">
        <h2 class="title">New timer</h2>

        <label class="row">
            <span class="row-label">Label</span>
            <input
                data-field="label"
                type="text"
                placeholder="Tea"
                bind:value={label}
            />
        </label>

        <div class="row mode-toggle" role="group" aria-label="Timer mode">
            <button
                type="button"
                data-mode="in"
                class:active={mode === 'in'}
                onclick={() => (mode = 'in')}>In</button
            >
            <button
                type="button"
                data-mode="at"
                class:active={mode === 'at'}
                onclick={() => (mode = 'at')}>At</button
            >
        </div>

        {#if mode === 'in'}
            <label class="row">
                <span class="row-label">Duration</span>
                <input
                    data-field="duration"
                    type="text"
                    placeholder="45m"
                    bind:value={durationText}
                />
            </label>
        {:else}
            <label class="row">
                <span class="row-label">Time</span>
                <input
                    data-field="time"
                    type="text"
                    placeholder="17:15"
                    bind:value={timeText}
                />
            </label>

            <div class="row" role="group" aria-label="Recurrence">
                <span class="row-label">Repeat</span>
                <div class="recurrence-toggle">
                    <button
                        type="button"
                        data-recurrence="none"
                        class:active={recurrence === 'none'}
                        onclick={() => (recurrence = 'none')}>Once</button
                    >
                    <button
                        type="button"
                        data-recurrence="daily"
                        class:active={recurrence === 'daily'}
                        onclick={() => (recurrence = 'daily')}>Daily</button
                    >
                    <button
                        type="button"
                        data-recurrence="custom"
                        class:active={recurrence === 'custom'}
                        onclick={() => (recurrence = 'custom')}>Custom</button
                    >
                </div>
            </div>

            <div class="row weekday-picker" role="group" aria-label="Weekdays">
                {#each ALL_WEEKDAYS as day (day)}
                    <button
                        type="button"
                        data-weekday={day}
                        class:active={customDays.includes(day)}
                        disabled={recurrence !== 'custom'}
                        onclick={() => toggleDay(day)}
                        title={day}
                    >
                        {day.slice(0, 3)}
                    </button>
                {/each}
            </div>
        {/if}

        <fieldset class="group">
            <legend>Alerting</legend>
            <label class="check">
                <input
                    data-field="notification"
                    type="checkbox"
                    bind:checked={notification}
                />
                <span>Notification</span>
            </label>
            <label class="check">
                <input data-field="sound" type="checkbox" bind:checked={soundOn} />
                <span>Sound</span>
            </label>
            <select
                data-field="sound-name"
                bind:value={soundName}
                disabled={!soundOn}
            >
                {#each SOUND_NAMES as name (name)}
                    <option value={name}>{name}</option>
                {/each}
            </select>
            <label class="check">
                <input
                    data-field="urgency-ramp"
                    type="checkbox"
                    bind:checked={urgencyRamp}
                />
                <span>Urgency ramp</span>
            </label>
        </fieldset>

        <fieldset class="group">
            <legend>Style</legend>
            <StylePicker
                style={visualStyle}
                accentHue={visualAccentHue}
                onChange={onStyleChange}
            />
        </fieldset>

        {#if error}
            <p class="form-error" role="alert">{error}</p>
        {/if}

        <div class="actions">
            <button type="button" data-action="cancel" class="secondary" onclick={close}>
                Cancel
            </button>
            <button type="button" data-action="submit" class="primary" onclick={submit}>
                Create
            </button>
        </div>
    </div>
</div>

<style>
    .backdrop {
        position: fixed;
        inset: 0;
        background: var(--color-overlay-backdrop, rgba(0, 0, 0, 0.5));
        backdrop-filter: blur(4px);
        display: flex;
        align-items: center;
        justify-content: center;
    }
    .card {
        background: var(--color-bg-alt, #313244);
        color: var(--color-fg, #cdd6f4);
        border-radius: 12px;
        padding: 18px 20px;
        width: 420px;
        box-shadow: 0 14px 40px var(--color-shadow, rgba(0, 0, 0, 0.6));
        border: 1px solid var(--color-border, #45475a);
    }
    .title {
        margin: 0 0 12px;
        font-size: 1.1rem;
    }
    .row {
        display: flex;
        align-items: center;
        gap: 10px;
        margin-bottom: 10px;
    }
    .row-label {
        width: 80px;
        flex-shrink: 0;
        color: var(--color-fg-muted, #a6adc8);
    }
    .row input {
        flex: 1;
        padding: 6px 8px;
        border-radius: 6px;
        border: 1px solid var(--color-border, #45475a);
        background: var(--color-bg, #1e1e2e);
        color: var(--color-fg, #cdd6f4);
    }
    .mode-toggle {
        gap: 6px;
    }
    .mode-toggle button {
        flex: 1;
        padding: 6px 10px;
        border-radius: 6px;
        border: 1px solid var(--color-border, #45475a);
        background: var(--color-bg, #1e1e2e);
        color: var(--color-fg, #cdd6f4);
        cursor: pointer;
    }
    .mode-toggle button.active {
        background: var(--color-accent, #89b4fa);
        color: var(--color-bg, #1e1e2e);
        border-color: var(--color-accent, #89b4fa);
    }
    .recurrence-toggle {
        display: flex;
        gap: 6px;
        flex: 1;
    }
    .recurrence-toggle button {
        flex: 1;
        padding: 6px 10px;
        border-radius: 6px;
        border: 1px solid var(--color-border, #45475a);
        background: var(--color-bg, #1e1e2e);
        color: var(--color-fg, #cdd6f4);
        cursor: pointer;
    }
    .recurrence-toggle button.active {
        background: var(--color-accent, #89b4fa);
        color: var(--color-bg, #1e1e2e);
        border-color: var(--color-accent, #89b4fa);
    }
    .weekday-picker {
        gap: 4px;
        flex-wrap: wrap;
    }
    .weekday-picker button {
        flex: 1;
        min-width: 38px;
        padding: 5px 4px;
        border-radius: 6px;
        border: 1px solid var(--color-border, #45475a);
        background: var(--color-bg, #1e1e2e);
        color: var(--color-fg, #cdd6f4);
        cursor: pointer;
        text-transform: capitalize;
    }
    .weekday-picker button.active {
        background: var(--color-accent, #89b4fa);
        color: var(--color-bg, #1e1e2e);
        border-color: var(--color-accent, #89b4fa);
    }
    .weekday-picker button:disabled {
        opacity: 0.4;
        cursor: not-allowed;
    }
    .group {
        border: 1px solid var(--color-border, #45475a);
        border-radius: 8px;
        padding: 10px 12px;
        margin: 0 0 10px;
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 12px;
    }
    .group legend {
        padding: 0 6px;
        color: var(--color-fg-muted, #a6adc8);
        font-size: 0.85rem;
    }
    .group :global(.style-picker) {
        flex: 1 1 100%;
    }
    .check {
        display: flex;
        align-items: center;
        gap: 6px;
        cursor: pointer;
    }
    .group select {
        padding: 5px 8px;
        border-radius: 6px;
        border: 1px solid var(--color-border, #45475a);
        background: var(--color-bg, #1e1e2e);
        color: var(--color-fg, #cdd6f4);
    }
    .group select:disabled {
        opacity: 0.4;
    }
    .form-error {
        margin: 6px 0;
        color: var(--color-error, #f38ba8);
        font-size: 0.9rem;
    }
    .actions {
        display: flex;
        justify-content: flex-end;
        gap: 8px;
        margin-top: 14px;
    }
    .actions button {
        padding: 7px 16px;
        border-radius: 6px;
        border: 1px solid var(--color-border, #45475a);
        cursor: pointer;
    }
    .actions .secondary {
        background: var(--color-bg, #1e1e2e);
        color: var(--color-fg, #cdd6f4);
    }
    .actions .primary {
        background: var(--color-accent, #89b4fa);
        color: var(--color-bg, #1e1e2e);
        border-color: var(--color-accent, #89b4fa);
    }
</style>
