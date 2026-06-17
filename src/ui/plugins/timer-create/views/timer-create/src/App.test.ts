import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte/svelte5';
import { tick } from 'svelte';
import type { NotifyConfig, VisualConfig, TimerStoreData } from '@quantum/client';

/*
 * Module-level mock state. Each test resets these via beforeEach so the
 * `vi.mock` factory below sees fresh values. The factory closes over them
 * by reference. Mirrors the power-menu App test pattern.
 */
let mockCallSpy = vi.fn();
let mockSubscribeSpy = vi.fn();

function defaultNotify(): NotifyConfig {
    return {
        notification: true,
        sound: 'complete',
        urgency_ramp: true,
        ramp_threshold: 0.1,
        pulse: false,
        flash: false,
    };
}

function defaultVisual(): VisualConfig {
    return {
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
}

function timerListResult(): TimerStoreData {
    return {
        settings: {
            layout: 'grid',
            gap: 12,
            align: 'center',
            defaults_visual: defaultVisual(),
            defaults_notify: defaultNotify(),
        },
        timers: [],
    };
}

vi.mock('@quantum/client', () => ({
    createClient: () => ({
        call: (...args: unknown[]) => {
            mockCallSpy(...args);
            const [method] = args as [string, unknown];
            if (method === 'timer.list') {
                return Promise.resolve(timerListResult());
            }
            return Promise.resolve(undefined);
        },
        subscribe: (...args: unknown[]) => {
            mockSubscribeSpy(...args);
            return () => {};
        },
        close: vi.fn(),
    }),
    __esModule: true,
}));

import App from './App.svelte';

beforeEach(() => {
    mockCallSpy = vi.fn();
    mockSubscribeSpy = vi.fn();
});

async function settle(): Promise<void> {
    await new Promise((resolve) => setTimeout(resolve, 10));
    await tick();
    await tick();
}

function createCallParams(): {
    label?: string;
    start?: unknown;
    visual?: VisualConfig;
    notify?: NotifyConfig;
} {
    const createCall = mockCallSpy.mock.calls.find(([method]) => method === 'timer.create');
    expect(createCall).toBeDefined();
    return createCall![1] as {
        label?: string;
        start?: unknown;
        visual?: VisualConfig;
        notify?: NotifyConfig;
    };
}

async function openAdvanced(container: HTMLElement): Promise<void> {
    const toggle = container.querySelector(
        '[data-action="toggle-advanced"]',
    ) as HTMLButtonElement;
    await fireEvent.click(toggle);
    await tick();
}

describe('TimerCreate App', () => {
    it('renders the backdrop and the centered card', () => {
        const { container } = render(App);
        expect(container.querySelector('.backdrop')).not.toBeNull();
        expect(container.querySelector('.card')).not.toBeNull();
    });

    it('keeps the Advanced section collapsed by default', async () => {
        const { container } = render(App);
        await settle();
        // The invert toggle lives inside Advanced; it should not be present
        // until Advanced is expanded.
        expect(container.querySelector('[data-field="invert"]')).toBeNull();
    });

    it('Escape key calls view.hide with the bare canonical name', async () => {
        render(App);
        await tick();
        await fireEvent.keyDown(document, { key: 'Escape' });
        await tick();
        const hidden = mockCallSpy.mock.calls.some(
            ([method, params]) =>
                method === 'view.hide' &&
                (params as { name?: string })?.name === 'plugin/timer-create/timer-create',
        );
        expect(hidden).toBe(true);
    });

    it('selecting the 45m chip submits a duration start of 2700 seconds', async () => {
        const { container } = render(App);
        await settle();

        const label = container.querySelector('[data-field="label"]') as HTMLInputElement;
        await fireEvent.input(label, { target: { value: 'Tea' } });
        await tick();

        const chip = container.querySelector('[data-chip="2700"]') as HTMLButtonElement;
        await fireEvent.click(chip);
        await tick();

        const submit = container.querySelector('[data-action="submit"]') as HTMLButtonElement;
        await fireEvent.click(submit);
        await settle();

        const params = createCallParams();
        expect(params.label).toBe('Tea');
        expect(params.start).toEqual({ kind: 'duration', secs: 2700 });

        const hidden = mockCallSpy.mock.calls.some(
            ([method, p]) =>
                method === 'view.hide' &&
                (p as { name?: string })?.name === 'plugin/timer-create/timer-create',
        );
        expect(hidden).toBe(true);
    });

    it('at mode: stepping hour/minute with PM produces a 0-23 hour', async () => {
        const { container } = render(App);
        await settle();

        const atMode = container.querySelector('[data-mode="at"]') as HTMLButtonElement;
        await fireEvent.click(atMode);
        await tick();

        // Default time is 09:00. Step hour +1 -> 10, minute +5 twice -> 10,
        // then choose PM -> 22:10.
        const hourUp = container.querySelector('[data-hour="1"]') as HTMLButtonElement;
        await fireEvent.click(hourUp);
        await tick();

        const minuteUp = container.querySelector('[data-minute="5"]') as HTMLButtonElement;
        await fireEvent.click(minuteUp);
        await fireEvent.click(minuteUp);
        await tick();

        const pm = container.querySelector('[data-period="pm"]') as HTMLButtonElement;
        await fireEvent.click(pm);
        await tick();

        const submit = container.querySelector('[data-action="submit"]') as HTMLButtonElement;
        await fireEvent.click(submit);
        await settle();

        const params = createCallParams();
        expect(params.start).toEqual({ kind: 'at', time: { hour: 22, minute: 10 } });
    });

    it('at mode + daily submits a recurring start with all seven days', async () => {
        const { container } = render(App);
        await settle();

        const atMode = container.querySelector('[data-mode="at"]') as HTMLButtonElement;
        await fireEvent.click(atMode);
        await tick();

        await openAdvanced(container);

        const daily = container.querySelector('[data-recurrence="daily"]') as HTMLButtonElement;
        await fireEvent.click(daily);
        await tick();

        const submit = container.querySelector('[data-action="submit"]') as HTMLButtonElement;
        await fireEvent.click(submit);
        await settle();

        const params = createCallParams();
        expect(params.start).toEqual({
            kind: 'recurring',
            days: [
                'monday',
                'tuesday',
                'wednesday',
                'thursday',
                'friday',
                'saturday',
                'sunday',
            ],
            time: { hour: 9, minute: 0 },
        });
    });

    it('toggling Invert on sends visual.fill true', async () => {
        const { container } = render(App);
        await settle();

        const chip = container.querySelector('[data-chip="600"]') as HTMLButtonElement;
        await fireEvent.click(chip);
        await tick();

        await openAdvanced(container);

        const invert = container.querySelector('[data-field="invert"]') as HTMLInputElement;
        await fireEvent.click(invert);
        await tick();

        const submit = container.querySelector('[data-action="submit"]') as HTMLButtonElement;
        await fireEvent.click(submit);
        await settle();

        const params = createCallParams();
        expect(params.visual).toBeDefined();
        expect(params.visual!.fill).toBe(true);
        // A complete object is sent: untouched default fields are present.
        expect(params.visual!.size).toBe(120);
    });

    it('selecting the pie swatch sends visual.style pie', async () => {
        const { container } = render(App);
        await settle();

        const chip = container.querySelector('[data-chip="600"]') as HTMLButtonElement;
        await fireEvent.click(chip);
        await tick();

        await openAdvanced(container);

        const pie = container.querySelector('[data-style="pie"]') as HTMLButtonElement;
        await fireEvent.click(pie);
        await tick();

        const submit = container.querySelector('[data-action="submit"]') as HTMLButtonElement;
        await fireEvent.click(submit);
        await settle();

        const params = createCallParams();
        expect(params.visual).toBeDefined();
        expect(params.visual!.style).toBe('pie');
    });

    it('turning Notification off sends a complete notify with notification false', async () => {
        const { container } = render(App);
        await settle();

        const listed = mockCallSpy.mock.calls.some(([method]) => method === 'timer.list');
        expect(listed).toBe(true);

        const chip = container.querySelector('[data-chip="600"]') as HTMLButtonElement;
        await fireEvent.click(chip);
        await tick();

        await openAdvanced(container);

        const notificationToggle = container.querySelector(
            '[data-field="notification"]',
        ) as HTMLInputElement;
        await fireEvent.click(notificationToggle);
        await tick();

        const submit = container.querySelector('[data-action="submit"]') as HTMLButtonElement;
        await fireEvent.click(submit);
        await settle();

        const params = createCallParams();
        expect(params.notify).toBeDefined();
        expect(params.notify!.notification).toBe(false);
        // A complete object is sent: untouched default fields are present.
        expect(params.notify!.ramp_threshold).toBe(0.1);
    });

    it('style picker offers exactly ring, pie, dots and bar swatches with no text labels', async () => {
        const { container } = render(App);
        await settle();
        await openAdvanced(container);

        const swatches = Array.from(
            container.querySelectorAll('[data-style]'),
        ) as HTMLElement[];
        const styles = swatches.map((swatch) => swatch.getAttribute('data-style'));
        expect(styles).toEqual(['ring', 'pie', 'dots', 'bar']);
        // Swatches are live SVG/CSS examples, not text labels.
        for (const swatch of swatches) {
            expect(swatch.textContent?.trim()).toBe('');
        }
    });

    it('each advanced row carries an info tooltip with real explanatory text', async () => {
        const { container } = render(App);
        await settle();

        const atMode = container.querySelector('[data-mode="at"]') as HTMLButtonElement;
        await fireEvent.click(atMode);
        await tick();
        await openAdvanced(container);

        const tips = Array.from(container.querySelectorAll('.info-tip')) as HTMLElement[];
        // Repeat, Notification, Sound, Urgency ramp, Direction, Style.
        expect(tips.length).toBeGreaterThanOrEqual(6);
        for (const tip of tips) {
            const bubble = tip.querySelector('.info-tip-bubble');
            expect(bubble?.textContent?.trim().length ?? 0).toBeGreaterThan(20);
        }
    });
});
