import { describe, it, expect, vi } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte/svelte5';
import Toolbar from './Toolbar.svelte';

function renderToolbar(extra: Partial<Record<string, unknown>> = {}) {
    return render(Toolbar, {
        props: {
            path: '/home/user',
            canGoBack: true,
            canGoForward: true,
            filter: '',
            deepSearch: false,
            dualPane: false,
            onNavigate: vi.fn(),
            onBack: vi.fn(),
            onForward: vi.fn(),
            onUp: vi.fn(),
            onFilterInput: vi.fn(),
            onToggleDeep: vi.fn(),
            onToggleDual: vi.fn(),
            onClose: vi.fn(),
            ...extra,
        },
    });
}

describe('Toolbar', () => {
    it('disables the back button when navigation history is exhausted', () => {
        const { container } = renderToolbar({ canGoBack: false });
        const back = container.querySelector('.b-back') as HTMLButtonElement;
        expect(back.disabled).toBe(true);
    });

    it('calls onBack when the enabled back button is clicked', async () => {
        const onBack = vi.fn();
        const { container } = renderToolbar({ canGoBack: true, onBack });
        const back = container.querySelector('.b-back') as HTMLButtonElement;
        expect(back.disabled).toBe(false);
        await fireEvent.click(back);
        expect(onBack).toHaveBeenCalledTimes(1);
    });

    it('disables the up button at the filesystem root', () => {
        const { container } = renderToolbar({ path: '/' });
        const up = container.querySelector('.b-up') as HTMLButtonElement;
        expect(up.disabled).toBe(true);
    });

    it('marks the deep chip active when deepSearch is on', () => {
        const { container } = renderToolbar({ deepSearch: true });
        const deep = container.querySelector('.deep') as HTMLElement;
        expect(deep.classList.contains('on')).toBe(true);
    });

    it('fires onFilterInput with the typed value', async () => {
        const onFilterInput = vi.fn();
        const { container } = renderToolbar({ onFilterInput });
        const input = container.querySelector('.filter-input') as HTMLInputElement;
        await fireEvent.input(input, { target: { value: 'report' } });
        expect(onFilterInput).toHaveBeenCalledWith('report');
    });

    it('gives every icon button a title tooltip', () => {
        const { container } = renderToolbar();
        for (const button of container.querySelectorAll('.icon-btn')) {
            expect(button.getAttribute('title')).toBeTruthy();
        }
    });

    it('renders a close button carrying a Close tooltip', () => {
        const { container } = renderToolbar();
        const close = container.querySelector('.b-close') as HTMLButtonElement;
        expect(close).not.toBeNull();
        expect(close.getAttribute('title')).toBe('Close (Alt+F4)');
        expect(close.getAttribute('aria-label')).toBeTruthy();
    });

    it('calls onClose when the close button is clicked', async () => {
        const onClose = vi.fn();
        const { container } = renderToolbar({ onClose });
        const close = container.querySelector('.b-close') as HTMLButtonElement;
        await fireEvent.click(close);
        expect(onClose).toHaveBeenCalledTimes(1);
    });
});
