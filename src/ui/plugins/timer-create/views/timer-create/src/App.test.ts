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

describe('TimerCreate App', () => {
    it('renders the backdrop and the centered card', () => {
        const { container } = render(App);
        expect(container.querySelector('.backdrop')).not.toBeNull();
        expect(container.querySelector('.card')).not.toBeNull();
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

    it('in mode: label + 45m submit calls timer.create then view.hide', async () => {
        const { container } = render(App);
        await settle();

        const label = container.querySelector('[data-field="label"]') as HTMLInputElement;
        await fireEvent.input(label, { target: { value: 'Tea' } });
        await tick();

        const duration = container.querySelector('[data-field="duration"]') as HTMLInputElement;
        await fireEvent.input(duration, { target: { value: '45m' } });
        await tick();

        const submit = container.querySelector('[data-action="submit"]') as HTMLButtonElement;
        await fireEvent.click(submit);
        await settle();

        const createCall = mockCallSpy.mock.calls.find(([method]) => method === 'timer.create');
        expect(createCall).toBeDefined();
        const createParams = createCall![1] as { label?: string; start?: unknown };
        expect(createParams.label).toBe('Tea');
        expect(createParams.start).toEqual({ kind: 'duration', secs: 2700 });

        const hidden = mockCallSpy.mock.calls.some(
            ([method, params]) =>
                method === 'view.hide' &&
                (params as { name?: string })?.name === 'plugin/timer-create/timer-create',
        );
        expect(hidden).toBe(true);
    });

    it('at mode + daily + 08:00 submits a recurring start', async () => {
        const { container } = render(App);
        await settle();

        const atMode = container.querySelector('[data-mode="at"]') as HTMLButtonElement;
        await fireEvent.click(atMode);
        await tick();

        const daily = container.querySelector('[data-recurrence="daily"]') as HTMLButtonElement;
        await fireEvent.click(daily);
        await tick();

        const time = container.querySelector('[data-field="time"]') as HTMLInputElement;
        await fireEvent.input(time, { target: { value: '08:00' } });
        await tick();

        const submit = container.querySelector('[data-action="submit"]') as HTMLButtonElement;
        await fireEvent.click(submit);
        await settle();

        const createCall = mockCallSpy.mock.calls.find(([method]) => method === 'timer.create');
        expect(createCall).toBeDefined();
        const params = createCall![1] as { start?: unknown };
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
            time: { hour: 8, minute: 0 },
        });
    });

    it('seeds defaults from timer.list and sends a complete notify with notification off', async () => {
        const { container } = render(App);
        await settle();

        const listed = mockCallSpy.mock.calls.some(([method]) => method === 'timer.list');
        expect(listed).toBe(true);

        const duration = container.querySelector('[data-field="duration"]') as HTMLInputElement;
        await fireEvent.input(duration, { target: { value: '10m' } });
        await tick();

        const notificationToggle = container.querySelector(
            '[data-field="notification"]',
        ) as HTMLInputElement;
        await fireEvent.click(notificationToggle);
        await tick();

        const submit = container.querySelector('[data-action="submit"]') as HTMLButtonElement;
        await fireEvent.click(submit);
        await settle();

        const createCall = mockCallSpy.mock.calls.find(([method]) => method === 'timer.create');
        expect(createCall).toBeDefined();
        const params = createCall![1] as { notify?: NotifyConfig; visual?: VisualConfig };
        expect(params.notify).toBeDefined();
        expect(params.notify!.notification).toBe(false);
        // A complete object is sent: untouched default fields are present.
        expect(params.notify!.ramp_threshold).toBe(0.1);
    });

    it('selecting style ring sends a complete visual with that style', async () => {
        const { container } = render(App);
        await settle();

        const duration = container.querySelector('[data-field="duration"]') as HTMLInputElement;
        await fireEvent.input(duration, { target: { value: '10m' } });
        await tick();

        const ring = container.querySelector('[data-style="ring"]') as HTMLButtonElement;
        await fireEvent.click(ring);
        await tick();

        const submit = container.querySelector('[data-action="submit"]') as HTMLButtonElement;
        await fireEvent.click(submit);
        await settle();

        const createCall = mockCallSpy.mock.calls.find(([method]) => method === 'timer.create');
        expect(createCall).toBeDefined();
        const params = createCall![1] as { visual?: VisualConfig };
        expect(params.visual).toBeDefined();
        expect(params.visual!.style).toBe('ring');
        // A complete object is sent: untouched default fields are present.
        expect(params.visual!.size).toBe(120);
    });
});
