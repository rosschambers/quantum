import { describe, it, expect, vi } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte/svelte5';
import { tick } from 'svelte';
import ActionRow from './ActionRow.svelte';

describe('ActionRow', () => {
    it('renders the label when not armed', () => {
        const onArm = vi.fn();
        const onInvoke = vi.fn();
        const { container } = render(ActionRow, {
            props: {
                command: 'suspend',
                label: 'Suspend',
                icon: 'power',
                armed: null,
                onArm,
                onInvoke,
            },
        });
        const button = container.querySelector('[data-command="suspend"]') as HTMLElement;
        expect(button.textContent).toContain('Suspend');
        expect(button.textContent).not.toContain('Confirm');
        expect(button.classList.contains('armed')).toBe(false);
    });

    it('renders the confirm label when armed', () => {
        const { container } = render(ActionRow, {
            props: {
                command: 'suspend',
                label: 'Suspend',
                icon: 'power',
                armed: 'suspend',
                onArm: vi.fn(),
                onInvoke: vi.fn(),
            },
        });
        const button = container.querySelector('[data-command="suspend"]') as HTMLElement;
        expect(button.textContent).toContain('Confirm Suspend');
        expect(button.classList.contains('armed')).toBe(true);
    });

    it('first click calls onArm with the command', async () => {
        const onArm = vi.fn();
        const onInvoke = vi.fn();
        const { container } = render(ActionRow, {
            props: {
                command: 'shutdown',
                label: 'Shutdown',
                icon: 'power',
                armed: null,
                onArm,
                onInvoke,
            },
        });
        const button = container.querySelector('[data-command="shutdown"]') as HTMLElement;
        await fireEvent.click(button);
        await tick();
        expect(onArm).toHaveBeenCalledWith('shutdown');
        expect(onInvoke).not.toHaveBeenCalled();
    });

    it('click when already armed calls onInvoke', async () => {
        const onArm = vi.fn();
        const onInvoke = vi.fn().mockResolvedValue(undefined);
        const { container } = render(ActionRow, {
            props: {
                command: 'lock',
                label: 'Lock',
                icon: 'power',
                armed: 'lock',
                onArm,
                onInvoke,
            },
        });
        const button = container.querySelector('[data-command="lock"]') as HTMLElement;
        await fireEvent.click(button);
        await tick();
        expect(onInvoke).toHaveBeenCalled();
        expect(onArm).not.toHaveBeenCalled();
    });

    it('renders an icon SVG', () => {
        const { container } = render(ActionRow, {
            props: {
                command: 'suspend',
                label: 'Suspend',
                icon: 'power',
                armed: null,
                onArm: vi.fn(),
                onInvoke: vi.fn(),
            },
        });
        expect(container.querySelector('svg.icon')).not.toBeNull();
    });
});
