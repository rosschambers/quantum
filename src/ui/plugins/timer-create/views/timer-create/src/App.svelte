<script lang="ts">
    import { createClient } from '@quantum/client';
    import { parseDurationToSecs, parseTimeOfDay } from './lib/parse';

    /** Bare canonical view name; the registry strips any `@<monitor>` suffix
     * for single-instance overlays, so `view.hide` uses the bare name. */
    const VIEW_NAME = 'plugin/timer-create/timer-create';

    const client = createClient();

    let label: string = $state('');
    let mode: 'in' | 'at' = $state('in');
    let durationText: string = $state('');
    let timeText: string = $state('');
    let error: string | null = $state(null);

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
        return { kind: 'at', time };
    }

    async function submit(): Promise<void> {
        error = null;
        const start = buildStart();
        if (start === null) return;
        try {
            await client.call('timer.create', { label, start });
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
        {/if}

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
