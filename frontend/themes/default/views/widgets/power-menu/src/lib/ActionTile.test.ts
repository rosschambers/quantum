import { describe, it, expect, vi } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte/svelte5';
import { tick } from 'svelte';
import ActionTile from './ActionTile.svelte';

describe('ActionTile', () => {
    it('renders the label when not armed', () => {
        const { container } = render(ActionTile, {
            props: {
                command: 'suspend',
                label: 'Suspend',
                icon: 'power',
                variant: 'secondary',
                armed: null,
                onArm: vi.fn(),
                onInvoke: vi.fn(),
            },
        });
        const tile = container.querySelector('[data-command="suspend"]') as HTMLElement;
        expect(tile.textContent).toContain('Suspend');
        expect(tile.textContent).not.toContain('Confirm');
        expect(tile.classList.contains('armed')).toBe(false);
    });

    it('renders the confirm label when armed', () => {
        const { container } = render(ActionTile, {
            props: {
                command: 'shutdown',
                label: 'Shutdown',
                icon: 'power',
                variant: 'primary',
                armed: 'shutdown',
                onArm: vi.fn(),
                onInvoke: vi.fn(),
            },
        });
        const tile = container.querySelector('[data-command="shutdown"]') as HTMLElement;
        expect(tile.textContent).toContain('Confirm Shutdown');
        expect(tile.classList.contains('armed')).toBe(true);
    });

    it('first click calls onArm with the command, never onInvoke', async () => {
        const onArm = vi.fn();
        const onInvoke = vi.fn();
        const { container } = render(ActionTile, {
            props: {
                command: 'shutdown',
                label: 'Shutdown',
                icon: 'power',
                variant: 'primary',
                armed: null,
                onArm,
                onInvoke,
            },
        });
        const tile = container.querySelector('[data-command="shutdown"]') as HTMLElement;
        await fireEvent.click(tile);
        await tick();
        expect(onArm).toHaveBeenCalledWith('shutdown');
        expect(onInvoke).not.toHaveBeenCalled();
    });

    it('click when already armed calls onInvoke, never onArm', async () => {
        const onArm = vi.fn();
        const onInvoke = vi.fn().mockResolvedValue(undefined);
        const { container } = render(ActionTile, {
            props: {
                command: 'lock',
                label: 'Lock',
                icon: 'bolt',
                variant: 'secondary',
                armed: 'lock',
                onArm,
                onInvoke,
            },
        });
        const tile = container.querySelector('[data-command="lock"]') as HTMLElement;
        await fireEvent.click(tile);
        await tick();
        expect(onInvoke).toHaveBeenCalled();
        expect(onArm).not.toHaveBeenCalled();
    });

    it('applies primary variant styling class', () => {
        const { container } = render(ActionTile, {
            props: {
                command: 'shutdown',
                label: 'Shutdown',
                icon: 'power',
                variant: 'primary',
                armed: null,
                onArm: vi.fn(),
                onInvoke: vi.fn(),
            },
        });
        const tile = container.querySelector('[data-command="shutdown"]') as HTMLElement;
        expect(tile.classList.contains('primary')).toBe(true);
        expect(tile.classList.contains('secondary')).toBe(false);
    });

    it('applies secondary variant styling class', () => {
        const { container } = render(ActionTile, {
            props: {
                command: 'lock',
                label: 'Lock',
                icon: 'bolt',
                variant: 'secondary',
                armed: null,
                onArm: vi.fn(),
                onInvoke: vi.fn(),
            },
        });
        const tile = container.querySelector('[data-command="lock"]') as HTMLElement;
        expect(tile.classList.contains('secondary')).toBe(true);
        expect(tile.classList.contains('primary')).toBe(false);
    });

    it('renders an icon SVG', () => {
        const { container } = render(ActionTile, {
            props: {
                command: 'suspend',
                label: 'Suspend',
                icon: 'sun',
                variant: 'secondary',
                armed: null,
                onArm: vi.fn(),
                onInvoke: vi.fn(),
            },
        });
        expect(container.querySelector('svg.icon')).not.toBeNull();
    });
});
