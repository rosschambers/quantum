import { describe, it, expect, vi } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte/svelte5';
import { tick } from 'svelte';
import DelayPicker from './DelayPicker.svelte';

describe('DelayPicker', () => {
    it('renders the trigger button with the Now label when value is 0', () => {
        const { container } = render(DelayPicker, {
            props: { value: 0, onChange: vi.fn() },
        });
        const trigger = container.querySelector('.trigger') as HTMLButtonElement;
        expect(trigger).not.toBeNull();
        expect(trigger.textContent).toContain('Now');
    });

    it('renders the trigger with the matching preset label for non-zero presets', () => {
        const { container } = render(DelayPicker, {
            props: { value: 30 * 60, onChange: vi.fn() },
        });
        const trigger = container.querySelector('.trigger') as HTMLButtonElement;
        expect(trigger.textContent).toContain('30 min');
    });

    it('renders a derived "<n> min" label when value is non-preset', () => {
        const { container } = render(DelayPicker, {
            props: { value: 7 * 60, onChange: vi.fn() },
        });
        const trigger = container.querySelector('.trigger') as HTMLButtonElement;
        expect(trigger.textContent).toContain('7 min');
    });

    it('popover is closed by default and opens on trigger click', async () => {
        const { container } = render(DelayPicker, {
            props: { value: 0, onChange: vi.fn() },
        });
        expect(container.querySelector('.popover')).toBeNull();
        const trigger = container.querySelector('.trigger') as HTMLButtonElement;
        await fireEvent.click(trigger);
        await tick();
        expect(container.querySelector('.popover')).not.toBeNull();
    });

    it('the open popover lists all the preset labels and a Custom entry', async () => {
        const { container } = render(DelayPicker, {
            props: { value: 0, onChange: vi.fn() },
        });
        const trigger = container.querySelector('.trigger') as HTMLButtonElement;
        await fireEvent.click(trigger);
        await tick();
        const options = Array.from(container.querySelectorAll('.option')).map(
            (option) => option.textContent?.trim() ?? '',
        );
        expect(options).toContain('Now');
        expect(options).toContain('5 min');
        expect(options).toContain('15 min');
        expect(options).toContain('30 min');
        expect(options).toContain('1 hour');
        expect(options).toContain('Custom…');
    });

    it('clicking a preset option calls onChange and closes the popover', async () => {
        const onChange = vi.fn();
        const { container } = render(DelayPicker, {
            props: { value: 0, onChange },
        });
        await fireEvent.click(container.querySelector('.trigger') as HTMLButtonElement);
        await tick();
        const fifteenMin = container.querySelector('[data-preset="900"]') as HTMLButtonElement;
        await fireEvent.click(fifteenMin);
        await tick();
        expect(onChange).toHaveBeenCalledWith(15 * 60);
        expect(container.querySelector('.popover')).toBeNull();
    });

    it('Custom entry reveals the custom-minutes input row inside the popover', async () => {
        const { container } = render(DelayPicker, {
            props: { value: 0, onChange: vi.fn() },
        });
        await fireEvent.click(container.querySelector('.trigger') as HTMLButtonElement);
        await tick();
        const customOption = container.querySelector('[data-preset="custom"]') as HTMLButtonElement;
        await fireEvent.click(customOption);
        await tick();
        expect(container.querySelector('.custom-input')).not.toBeNull();
        // Preset options should no longer be visible.
        expect(container.querySelector('[data-preset="900"]')).toBeNull();
    });

    it('custom OK confirms with clamped minutes converted to seconds', async () => {
        const onChange = vi.fn();
        const { container } = render(DelayPicker, {
            props: { value: 0, onChange },
        });
        await fireEvent.click(container.querySelector('.trigger') as HTMLButtonElement);
        await tick();
        await fireEvent.click(
            container.querySelector('[data-preset="custom"]') as HTMLButtonElement,
        );
        await tick();

        const input = container.querySelector('.custom-input') as HTMLInputElement;
        await fireEvent.input(input, { target: { value: '2000' } });
        await tick();

        const okButton = Array.from(container.querySelectorAll('.pill')).find(
            (pill) => pill.textContent?.trim() === 'OK',
        ) as HTMLElement;
        await fireEvent.click(okButton);
        await tick();
        expect(onChange).toHaveBeenCalledWith(1440 * 60);
        // Popover should close after a successful confirm.
        expect(container.querySelector('.popover')).toBeNull();
    });

    it('custom Cancel reverts to the preset list without calling onChange', async () => {
        const onChange = vi.fn();
        const { container } = render(DelayPicker, {
            props: { value: 0, onChange },
        });
        await fireEvent.click(container.querySelector('.trigger') as HTMLButtonElement);
        await tick();
        await fireEvent.click(
            container.querySelector('[data-preset="custom"]') as HTMLButtonElement,
        );
        await tick();

        const cancelButton = Array.from(container.querySelectorAll('.pill')).find(
            (pill) => pill.textContent?.trim() === 'Cancel',
        ) as HTMLElement;
        await fireEvent.click(cancelButton);
        await tick();
        // Back on the preset list, popover still open.
        expect(container.querySelector('.popover')).not.toBeNull();
        expect(container.querySelector('[data-preset="900"]')).not.toBeNull();
        expect(container.querySelector('.custom-input')).toBeNull();
        expect(onChange).not.toHaveBeenCalled();
    });

    it('marks the option matching the current value as selected', async () => {
        const { container } = render(DelayPicker, {
            props: { value: 30 * 60, onChange: vi.fn() },
        });
        await fireEvent.click(container.querySelector('.trigger') as HTMLButtonElement);
        await tick();
        const selected = container.querySelector('.option.selected') as HTMLElement;
        expect(selected).not.toBeNull();
        expect(selected.textContent?.trim()).toBe('30 min');
    });
});
