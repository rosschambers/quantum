import { describe, it, expect, vi } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte/svelte5';
import SearchBar from './SearchBar.svelte';

function renderBar(extra: Partial<Record<string, unknown>> = {}) {
    return render(SearchBar, {
        props: {
            query: '',
            totalMatches: 0,
            currentMatchIndex: null,
            onQueryInput: vi.fn(),
            onNext: vi.fn(),
            onPrevious: vi.fn(),
            onClose: vi.fn(),
            ...extra,
        },
    });
}

describe('SearchBar', () => {
    it('renders a text input', () => {
        const { container } = renderBar();
        expect(container.querySelector('.search-bar input')).not.toBeNull();
    });

    it('shows "No matches" when the query is non-empty and there are no matches', () => {
        const { container } = renderBar({ query: 'xyz', totalMatches: 0, currentMatchIndex: null });
        expect(container.querySelector('.match-indicator')?.textContent).toBe('No matches');
    });

    it('shows no indicator text when the query is empty', () => {
        const { container } = renderBar({ query: '', totalMatches: 0, currentMatchIndex: null });
        expect(container.querySelector('.match-indicator')?.textContent).toBe('');
    });

    it('shows "1 of 3" style match position, one-based', () => {
        const { container } = renderBar({ query: 'a', totalMatches: 3, currentMatchIndex: 0 });
        expect(container.querySelector('.match-indicator')?.textContent).toBe('1 of 3');

        const second = renderBar({ query: 'a', totalMatches: 3, currentMatchIndex: 2 });
        expect(second.container.querySelector('.match-indicator')?.textContent).toBe('3 of 3');
    });

    it('fires onQueryInput with the typed value', async () => {
        const onQueryInput = vi.fn();
        const { container } = renderBar({ onQueryInput });
        const input = container.querySelector('.search-bar input') as HTMLInputElement;
        await fireEvent.input(input, { target: { value: 'needle' } });
        expect(onQueryInput).toHaveBeenCalledWith('needle');
    });

    it('calls onNext / onPrevious when the next / previous buttons are clicked', async () => {
        const onNext = vi.fn();
        const onPrevious = vi.fn();
        const { container } = renderBar({ onNext, onPrevious, totalMatches: 2, currentMatchIndex: 0 });
        await fireEvent.click(container.querySelector('.search-next') as HTMLButtonElement);
        expect(onNext).toHaveBeenCalledTimes(1);
        await fireEvent.click(container.querySelector('.search-previous') as HTMLButtonElement);
        expect(onPrevious).toHaveBeenCalledTimes(1);
    });

    it('calls onClose when the close button is clicked', async () => {
        const onClose = vi.fn();
        const { container } = renderBar({ onClose });
        await fireEvent.click(container.querySelector('.search-close') as HTMLButtonElement);
        expect(onClose).toHaveBeenCalledTimes(1);
    });

    it('focuses the input on initial mount, unlike the always-mounted Toolbar filter input', () => {
        // SearchBar is only ever mounted by App.svelte opening search (it
        // does not persist across the whole app's lifetime like the
        // explorer Toolbar), so every mount — including the very first
        // effect run — corresponds to a real "focus this" request.
        const { container } = renderBar({ focusSignal: 0 });
        const input = container.querySelector('.search-bar input');
        expect(document.activeElement).toBe(input);
    });

    it('focuses and selects the input text again when focusSignal changes while already mounted', async () => {
        const { container, rerender } = renderBar({ query: 'needle', focusSignal: 0 });
        const input = container.querySelector('.search-bar input') as HTMLInputElement;
        // Mount itself already focused it (see the test above); move focus
        // elsewhere first so this test actually exercises the CHANGE path,
        // not just the already-covered mount path.
        input.blur();
        expect(document.activeElement).not.toBe(input);

        await rerender({
            query: 'needle', totalMatches: 0, currentMatchIndex: null,
            onQueryInput: vi.fn(), onNext: vi.fn(), onPrevious: vi.fn(), onClose: vi.fn(),
            focusSignal: 1,
        });

        expect(document.activeElement).toBe(input);
        expect(input.selectionStart).toBe(0);
        expect(input.selectionEnd).toBe(input.value.length);
    });
});
