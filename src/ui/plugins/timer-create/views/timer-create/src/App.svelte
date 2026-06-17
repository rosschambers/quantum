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
    import { DURATION_PRESETS, formatDuration, stepDuration } from './lib/duration';
    import { to12Hour, setPeriod } from './lib/time';
    import { ALL_WEEKDAYS, recurrenceDays, type Recurrence } from './lib/recurrence';
    import StylePicker from './lib/StylePicker.svelte';
    import InfoTip from './lib/InfoTip.svelte';

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
        style: 'ring',
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

    /** Step sizes for the custom controls. */
    const DURATION_STEP_SECS = 5 * 60;
    const MINUTE_STEP = 5;

    /** Per-row tooltip copy for the Advanced section. */
    const TIP_REPEAT =
        'How often this timer fires. Once = a single timer. Daily = every day at this time. Days… = only the weekdays you pick.';
    const TIP_NOTIFICATION = 'Show a desktop notification when the timer reaches zero.';
    const TIP_SOUND = 'Play a sound when the timer finishes.';
    const TIP_URGENCY =
        "Fade the timer's colour toward red as it nears zero so it catches your eye.";
    const TIP_DIRECTION =
        'By default the visual depletes (full → empty) as time runs out. Invert to count up (empty → full) instead.';
    const TIP_STYLE = 'The shape used to show the time remaining.';

    const client = createClient();

    let label: string = $state('');
    let mode: 'in' | 'at' = $state('in');

    // Duration ("in") state, in seconds.
    let durationSecs: number = $state(900);

    // Absolute time ("at") state. Hour is stored 0-23, minute 0-59.
    let hour: number = $state(9);
    let minute: number = $state(0);

    let recurrence: Recurrence = $state('none');
    let customDays: Weekday[] = $state([]);
    let advancedOpen: boolean = $state(false);
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

    // Direction: invert maps to visual.fill (true = count up / fill).
    let invert: boolean = $state(false);

    // Visual style, fed from StylePicker.
    let visualStyle: VisualStyle = $state('ring');

    const period = $derived(to12Hour(hour).period);
    const hourDisplay = $derived(to12Hour(hour).hour12);

    function selectPreset(secs: number): void {
        durationSecs = secs;
    }

    function stepDurationBy(delta: number): void {
        durationSecs = stepDuration(durationSecs, delta);
    }

    function stepHour(delta: number): void {
        hour = (hour + delta + 24) % 24;
    }

    function stepMinute(delta: number): void {
        minute = (minute + delta + 60) % 60;
    }

    function choosePeriod(next: 'am' | 'pm'): void {
        hour = setPeriod(hour, next);
    }

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
        visualStyle = visual.style === 'mixed' || visual.style === 'wedge' ? 'ring' : visual.style;
        invert = visual.fill;
    }

    function onStyleChange(next: VisualStyle): void {
        visualStyle = next;
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
     * and return `null` when the inputs are invalid. */
    function buildStart(): Record<string, unknown> | null {
        if (mode === 'in') {
            return { kind: 'duration', secs: durationSecs };
        }
        const time = { hour, minute };
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
            fill: invert,
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

        <div class="field">
            <label class="field-label" for="timer-label">Label</label>
            <input
                id="timer-label"
                data-field="label"
                class="text-input"
                type="text"
                placeholder="e.g. Walk the dog"
                bind:value={label}
            />
        </div>

        <div class="field">
            <div class="segmented" role="group" aria-label="Timer mode">
                <button
                    type="button"
                    data-mode="in"
                    class:on={mode === 'in'}
                    onclick={() => (mode = 'in')}>In</button
                >
                <button
                    type="button"
                    data-mode="at"
                    class:on={mode === 'at'}
                    onclick={() => (mode = 'at')}>At a time</button
                >
            </div>
        </div>

        {#if mode === 'in'}
            <div class="field">
                <span class="field-label">Duration</span>
                <div class="chips" role="group" aria-label="Duration presets">
                    {#each DURATION_PRESETS as preset (preset.secs)}
                        <button
                            type="button"
                            class="chip"
                            class:on={durationSecs === preset.secs}
                            data-chip={preset.secs}
                            onclick={() => selectPreset(preset.secs)}>{preset.label}</button
                        >
                    {/each}
                </div>
                <div class="stepper" aria-label="Custom duration">
                    <button
                        type="button"
                        data-step={-DURATION_STEP_SECS}
                        onclick={() => stepDurationBy(-DURATION_STEP_SECS)}>−</button
                    >
                    <span class="stepper-display" data-field="duration-display"
                        >{formatDuration(durationSecs)}</span
                    >
                    <button
                        type="button"
                        data-step={DURATION_STEP_SECS}
                        onclick={() => stepDurationBy(DURATION_STEP_SECS)}>+</button
                    >
                </div>
            </div>
        {:else}
            <div class="field">
                <span class="field-label">Time</span>
                <div class="time-row">
                    <div class="stepper" aria-label="Hour">
                        <button type="button" data-hour={-1} onclick={() => stepHour(-1)}
                            >−</button
                        >
                        <span class="stepper-display" data-field="hour-display"
                            >{hourDisplay}<span class="unit"> h</span></span
                        >
                        <button type="button" data-hour={1} onclick={() => stepHour(1)}
                            >+</button
                        >
                    </div>
                    <span class="colon">:</span>
                    <div class="stepper" aria-label="Minute">
                        <button
                            type="button"
                            data-minute={-MINUTE_STEP}
                            onclick={() => stepMinute(-MINUTE_STEP)}>−</button
                        >
                        <span class="stepper-display" data-field="minute-display"
                            >{String(minute).padStart(2, '0')}<span class="unit"> m</span></span
                        >
                        <button
                            type="button"
                            data-minute={MINUTE_STEP}
                            onclick={() => stepMinute(MINUTE_STEP)}>+</button
                        >
                    </div>
                    <div class="ampm" role="group" aria-label="AM or PM">
                        <button
                            type="button"
                            data-period="am"
                            class:on={period === 'am'}
                            onclick={() => choosePeriod('am')}>AM</button
                        >
                        <button
                            type="button"
                            data-period="pm"
                            class:on={period === 'pm'}
                            onclick={() => choosePeriod('pm')}>PM</button
                        >
                    </div>
                </div>
            </div>
        {/if}

        <div class="advanced">
            <button
                type="button"
                class="advanced-summary"
                data-action="toggle-advanced"
                aria-expanded={advancedOpen}
                onclick={() => (advancedOpen = !advancedOpen)}
            >
                <span class="advanced-caret">{advancedOpen ? '▾' : '▸'}</span> Advanced
            </button>

            {#if advancedOpen}
                <div class="advanced-body">
                    {#if mode === 'at'}
                        <div class="field" role="group" aria-label="Repeat">
                            <span class="field-label"
                                >Repeat <InfoTip text={TIP_REPEAT} /></span
                            >
                            <div class="segmented">
                                <button
                                    type="button"
                                    data-recurrence="none"
                                    class:on={recurrence === 'none'}
                                    onclick={() => (recurrence = 'none')}>Once</button
                                >
                                <button
                                    type="button"
                                    data-recurrence="daily"
                                    class:on={recurrence === 'daily'}
                                    onclick={() => (recurrence = 'daily')}>Daily</button
                                >
                                <button
                                    type="button"
                                    data-recurrence="custom"
                                    class:on={recurrence === 'custom'}
                                    onclick={() => (recurrence = 'custom')}>Days…</button
                                >
                            </div>
                            {#if recurrence === 'custom'}
                                <div class="weekdays" role="group" aria-label="Weekdays">
                                    {#each ALL_WEEKDAYS as day (day)}
                                        <button
                                            type="button"
                                            data-weekday={day}
                                            class:on={customDays.includes(day)}
                                            onclick={() => toggleDay(day)}
                                            title={day}
                                        >
                                            {day.slice(0, 1).toUpperCase()}{day.slice(1, 2)}
                                        </button>
                                    {/each}
                                </div>
                            {/if}
                        </div>
                    {/if}

                    <div class="field">
                        <span class="field-label">Alerts</span>
                        <div class="alert-row">
                            <span class="alert-label"
                                >Notification <InfoTip text={TIP_NOTIFICATION} /></span
                            >
                            <input
                                data-field="notification"
                                type="checkbox"
                                bind:checked={notification}
                            />
                        </div>
                        <div class="alert-row">
                            <span class="alert-label"
                                >Sound <InfoTip text={TIP_SOUND} /></span
                            >
                            <div class="alert-control">
                                <select
                                    data-field="sound-name"
                                    bind:value={soundName}
                                    disabled={!soundOn}
                                >
                                    {#each SOUND_NAMES as name (name)}
                                        <option value={name}>{name}</option>
                                    {/each}
                                </select>
                                <input
                                    data-field="sound"
                                    type="checkbox"
                                    bind:checked={soundOn}
                                />
                            </div>
                        </div>
                        <div class="alert-row">
                            <span class="alert-label"
                                >Urgency ramp <InfoTip text={TIP_URGENCY} /></span
                            >
                            <input
                                data-field="urgency-ramp"
                                type="checkbox"
                                bind:checked={urgencyRamp}
                            />
                        </div>
                    </div>

                    <div class="field">
                        <span class="field-label">Direction</span>
                        <div class="alert-row">
                            <span class="alert-label"
                                >Invert — count up / fill <InfoTip text={TIP_DIRECTION} /></span
                            >
                            <input data-field="invert" type="checkbox" bind:checked={invert} />
                        </div>
                    </div>

                    <div class="field">
                        <span class="field-label">Style <InfoTip text={TIP_STYLE} /></span>
                        <StylePicker
                            style={visualStyle}
                            accentHue={defaultsVisual.accent_hue}
                            onChange={onStyleChange}
                        />
                    </div>
                </div>
            {/if}
        </div>

        {#if error}
            <p class="form-error" role="alert">{error}</p>
        {/if}

        <button type="button" data-action="submit" class="create-button" onclick={submit}>
            Create timer
        </button>
    </div>
</div>

<style>
    .backdrop {
        position: fixed;
        inset: 0;
        background: var(--color-overlay-backdrop, rgba(0, 0, 0, 0.45));
        backdrop-filter: blur(4px);
        display: flex;
        align-items: center;
        justify-content: center;
    }
    .card {
        background: var(--color-bg-alt, #161b26);
        color: var(--color-fg, #e6e9ef);
        border-radius: 16px;
        padding: 22px;
        width: 420px;
        display: flex;
        flex-direction: column;
        gap: 16px;
        box-shadow: 0 24px 60px var(--color-shadow, rgba(0, 0, 0, 0.5));
        border: 1px solid var(--color-border, #2a3142);
    }
    .title {
        margin: 0;
        font-size: 1.05rem;
        font-weight: 600;
    }
    .field {
        display: flex;
        flex-direction: column;
        gap: 7px;
    }
    .field-label {
        display: flex;
        align-items: center;
        gap: 6px;
        font-size: 0.75rem;
        color: var(--color-fg-muted, #8b94a7);
    }
    .text-input {
        background: var(--color-bg, #1c2230);
        border: 1px solid var(--color-border, #2a3142);
        color: var(--color-fg, #e6e9ef);
        border-radius: 9px;
        padding: 10px 12px;
        font-size: 0.9rem;
    }
    .text-input:focus {
        outline: none;
        border-color: var(--color-accent, #5b9dff);
    }
    .segmented {
        display: flex;
        background: var(--color-bg, #1c2230);
        border: 1px solid var(--color-border, #2a3142);
        border-radius: 10px;
        padding: 3px;
        gap: 3px;
    }
    .segmented button {
        flex: 1;
        border: none;
        background: transparent;
        color: var(--color-fg-muted, #8b94a7);
        font-size: 0.85rem;
        padding: 8px;
        border-radius: 7px;
        cursor: pointer;
    }
    .segmented button.on {
        background: var(--color-accent, #5b9dff);
        color: var(--color-bg, #08111f);
        font-weight: 600;
    }
    .chips {
        display: flex;
        flex-wrap: wrap;
        gap: 8px;
    }
    .chip {
        border: 1px solid var(--color-border, #2a3142);
        background: var(--color-bg, #1c2230);
        color: var(--color-fg, #e6e9ef);
        border-radius: 999px;
        padding: 8px 14px;
        font-size: 0.82rem;
        cursor: pointer;
    }
    .chip:hover {
        border-color: var(--color-accent, #5b9dff);
    }
    .chip.on {
        border-color: var(--color-accent, #5b9dff);
        background: var(--color-accent-soft, rgba(91, 157, 255, 0.15));
        color: var(--color-accent, #cfe0ff);
    }
    .stepper {
        display: flex;
        align-items: center;
        border: 1px solid var(--color-border, #2a3142);
        border-radius: 10px;
        overflow: hidden;
        width: fit-content;
        background: var(--color-bg, #1c2230);
    }
    .stepper button {
        border: none;
        background: transparent;
        color: var(--color-fg, #e6e9ef);
        font-size: 1.1rem;
        width: 42px;
        height: 44px;
        cursor: pointer;
    }
    .stepper button:hover {
        background: rgba(255, 255, 255, 0.06);
    }
    .stepper-display {
        min-width: 84px;
        text-align: center;
        font-family: var(--font-mono, ui-monospace, monospace);
        font-size: 1rem;
        padding: 0 6px;
    }
    .unit {
        color: var(--color-fg-muted, #8b94a7);
        font-size: 0.75rem;
    }
    .time-row {
        display: flex;
        align-items: center;
        gap: 12px;
    }
    .colon {
        font-size: 1.4rem;
        color: var(--color-fg-muted, #8b94a7);
    }
    .ampm {
        display: flex;
        flex-direction: column;
        border: 1px solid var(--color-border, #2a3142);
        border-radius: 10px;
        overflow: hidden;
    }
    .ampm button {
        border: none;
        background: var(--color-bg, #1c2230);
        color: var(--color-fg-muted, #8b94a7);
        font-size: 0.75rem;
        padding: 8px 12px;
        cursor: pointer;
    }
    .ampm button.on {
        background: var(--color-accent, #5b9dff);
        color: var(--color-bg, #08111f);
        font-weight: 600;
    }
    .advanced {
        border-top: 1px solid var(--color-border, #2a3142);
        padding-top: 6px;
    }
    .advanced-summary {
        background: none;
        border: none;
        color: var(--color-accent, #5b9dff);
        font-size: 0.85rem;
        padding: 6px 0;
        cursor: pointer;
        user-select: none;
    }
    .advanced-caret {
        display: inline-block;
        width: 1em;
    }
    .advanced-body {
        display: flex;
        flex-direction: column;
        gap: 16px;
        padding-top: 8px;
    }
    .weekdays {
        display: flex;
        gap: 6px;
        flex-wrap: wrap;
    }
    .weekdays button {
        width: 38px;
        height: 34px;
        border: 1px solid var(--color-border, #2a3142);
        background: var(--color-bg, #1c2230);
        color: var(--color-fg-muted, #8b94a7);
        border-radius: 8px;
        font-size: 0.75rem;
        cursor: pointer;
    }
    .weekdays button.on {
        border-color: var(--color-accent, #5b9dff);
        background: var(--color-accent-soft, rgba(91, 157, 255, 0.15));
        color: var(--color-accent, #cfe0ff);
    }
    .alert-row {
        display: flex;
        align-items: center;
        justify-content: space-between;
        font-size: 0.85rem;
    }
    .alert-label {
        display: flex;
        align-items: center;
        gap: 6px;
    }
    .alert-control {
        display: flex;
        align-items: center;
        gap: 8px;
    }
    .alert-control select {
        padding: 5px 8px;
        border-radius: 6px;
        border: 1px solid var(--color-border, #2a3142);
        background: var(--color-bg, #1c2230);
        color: var(--color-fg, #e6e9ef);
    }
    .alert-control select:disabled {
        opacity: 0.4;
    }
    .form-error {
        margin: 0;
        color: var(--color-error, #f38ba8);
        font-size: 0.85rem;
    }
    .create-button {
        background: var(--color-accent, #5b9dff);
        color: var(--color-bg, #08111f);
        border: none;
        border-radius: 10px;
        padding: 12px;
        font-size: 0.9rem;
        font-weight: 600;
        cursor: pointer;
        margin-top: 4px;
    }
</style>
