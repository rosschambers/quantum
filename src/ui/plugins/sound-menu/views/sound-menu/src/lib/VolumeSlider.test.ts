import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte/svelte5';
import VolumeSlider from './VolumeSlider.svelte';

afterEach(() => {
    vi.useRealTimers();
});

describe('VolumeSlider', () => {
    it('renders a 0-150 range at the given percent', () => {
        const { container } = render(VolumeSlider, {
            props: { percent: 55, onCommit: vi.fn() },
        });
        const slider = container.querySelector('input[type="range"]') as HTMLInputElement;
        expect(slider).not.toBeNull();
        expect(slider.min).toBe('0');
        expect(slider.max).toBe('150');
        expect(slider.value).toBe('55');
    });

    it('debounces rapid drag input into one trailing commit', async () => {
        vi.useFakeTimers();
        const onCommit = vi.fn();
        const { container } = render(VolumeSlider, { props: { percent: 55, onCommit } });
        const slider = container.querySelector('input[type="range"]') as HTMLInputElement;

        await fireEvent.input(slider, { target: { value: '60' } });
        await fireEvent.input(slider, { target: { value: '70' } });
        await fireEvent.input(slider, { target: { value: '80' } });
        expect(onCommit).not.toHaveBeenCalled();

        vi.advanceTimersByTime(150);
        expect(onCommit).toHaveBeenCalledTimes(1);
        expect(onCommit).toHaveBeenCalledWith(80);
    });

    it('commits immediately on release, cancelling the pending debounce', async () => {
        vi.useFakeTimers();
        const onCommit = vi.fn();
        const { container } = render(VolumeSlider, { props: { percent: 55, onCommit } });
        const slider = container.querySelector('input[type="range"]') as HTMLInputElement;

        await fireEvent.input(slider, { target: { value: '90' } });
        await fireEvent.change(slider, { target: { value: '90' } });
        expect(onCommit).toHaveBeenCalledTimes(1);
        expect(onCommit).toHaveBeenCalledWith(90);

        vi.advanceTimersByTime(300);
        expect(onCommit).toHaveBeenCalledTimes(1);
    });

    it('local-echoes while dragging and reconciles to props after release', async () => {
        vi.useFakeTimers();
        const onCommit = vi.fn();
        const { container, rerender } = render(VolumeSlider, {
            props: { percent: 55, onCommit },
        });
        const slider = container.querySelector('input[type="range"]') as HTMLInputElement;

        // Mid-drag, an event storm updates props; the drag value must win.
        await fireEvent.input(slider, { target: { value: '70' } });
        await rerender({ percent: 40, onCommit });
        expect(slider.value).toBe('70');

        // After release, provider state is the truth again.
        await fireEvent.change(slider, { target: { value: '70' } });
        await rerender({ percent: 42, onCommit });
        expect(slider.value).toBe('42');
    });
});
