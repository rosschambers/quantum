import { describe, it, expect, vi, afterEach, beforeAll } from 'vitest';
import { render, fireEvent, cleanup } from '@testing-library/svelte/svelte5';

const { callMock } = vi.hoisted(() => ({ callMock: vi.fn() }));
vi.mock('@quantum/client', async (importOriginal) => {
    const actual = await importOriginal<Record<string, unknown>>();
    return { ...actual, createClient: () => ({ call: callMock, subscribe: vi.fn(), close: vi.fn() }) };
});

import App from './App.svelte';

vi.mock('mermaid', () => ({ default: {
    initialize: vi.fn(),
    render: vi.fn(async () => ({ svg: '<svg><text>needle diagram</text></svg>' })),
} }));

beforeAll(() => {
    if (typeof (globalThis as any).ResizeObserver === 'undefined') {
        (globalThis as any).ResizeObserver = class {
            observe(): void {}
            unobserve(): void {}
            disconnect(): void {}
        };
    }
    if (typeof Element.prototype.scrollIntoView !== 'function') {
        Element.prototype.scrollIntoView = function (): void {};
    }
});

afterEach(() => {
    // The testing-library svelte5 adapter only auto-registers its own
    // afterEach cleanup when a GLOBAL `afterEach` exists (`test.globals` is
    // not enabled in this plugin's vite.config.ts) — without an explicit
    // call here, App's window-level keydown listener from a previous test
    // would leak into the next one, double-firing `callMock` and producing
    // exactly the kind of cross-test contamination "repeated mounts" guards
    // against.
    cleanup();
    delete (window as any).__quantum_args;
    callMock.mockReset();
});

type FixtureFileInfo = {
    content: string;
    file_type: 'text' | 'code' | 'json' | 'markdown';
    language?: string | null;
};

async function mountFile(fixture: FixtureFileInfo) {
    (window as any).__quantum_args = { path: '/tmp/fixture' };
    callMock.mockImplementation((method: string) => {
        if (method === 'file-viewer.read') {
            return Promise.resolve({
                content: fixture.content,
                file_type: fixture.file_type,
                filename: 'fixture',
                directory: '/tmp',
                size: fixture.content.length,
                language: fixture.language ?? null,
                mime_type: null,
                uri: null,
            });
        }
        return Promise.resolve(undefined);
    });

    const result = render(App);
    await vi.waitFor(() => {
        expect(callMock).toHaveBeenCalledWith('file-viewer.read', { path: '/tmp/fixture' });
    });
    // Let the content-area render past the loading state.
    await vi.waitFor(() => {
        expect(result.container.querySelector('.content-area')).not.toBeNull();
    });
    return result;
}

describe('App keyboard handling', () => {
    it.each(['.search-close', '.search-previous', '.search-next'])('preserves native Enter activation on focused %s', async (selector) => {
        const { container } = await mountFile({ content: 'needle needle needle', file_type: 'text' });
        await fireEvent.keyDown(window, { key: 'f', ctrlKey: true });
        await fireEvent.input(container.querySelector('.search-input')!, { target: { value: 'needle' } });
        const button = container.querySelector(selector) as HTMLButtonElement;
        button.focus();
        const event = new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true });
        await fireEvent(button, event);
        expect(event.defaultPrevented).toBe(false);
        expect(container.querySelector('.match-indicator')?.textContent).toBe('1 of 3');
        // jsdom does not synthesize the browser's Enter-to-click default action.
        await fireEvent.click(button);
        if (selector === '.search-close') expect(container.querySelector('.search-bar')).toBeNull();
        else expect(container.querySelector('.match-indicator')?.textContent).toBe(selector === '.search-previous' ? '3 of 3' : '2 of 3');
    });

    it('preserves Enter on links and editable controls but owns search-input and document Enter', async () => {
        const { container } = await mountFile({ file_type: 'markdown', content: '[needle](/target) needle\n\n<textarea>editable</textarea>' });
        await fireEvent.keyDown(window, { key: 'f', ctrlKey: true });
        const input = container.querySelector('.search-input')!;
        await fireEvent.input(input, { target: { value: 'needle' } });
        for (const selector of ['.markdown-renderer a', 'textarea']) {
            const target = container.querySelector(selector) as HTMLElement;
            target.focus();
            const event = new KeyboardEvent('keydown', { key: 'Enter', shiftKey: true, bubbles: true, cancelable: true });
            await fireEvent(target, event);
            expect(event.defaultPrevented).toBe(false);
            expect(container.querySelector('.match-indicator')?.textContent).toBe('1 of 2');
        }
        await fireEvent.keyDown(input, { key: 'Enter' });
        expect(container.querySelector('.match-indicator')?.textContent).toBe('2 of 2');
        await fireEvent.keyDown(container.querySelector('.markdown-renderer p')!, { key: 'Enter' });
        expect(container.querySelector('.match-indicator')?.textContent).toBe('1 of 2');
        await fireEvent.keyDown(container.querySelector('textarea')!, { key: 'Escape' });
        expect(container.querySelector('.search-bar')).toBeNull();
    });
    describe.each(['json', 'markdown'] as const)('%s single-match reveal', (file_type) => {
        it.each(['Enter', 'Shift+Enter', 'next button', 'previous button'])('repeats %s after manually scrolling away without changing selection', async (navigation) => {
            const { container } = await mountFile({
                file_type,
                content: file_type === 'json' ? '{"value":"needle"}' : 'Find the needle in prose.',
            });
            await fireEvent.keyDown(window, { key: 'f', ctrlKey: true });
            await fireEvent.input(container.querySelector('.search-bar input')!, { target: { value: 'needle' } });
            await vi.waitFor(() => expect(container.querySelector('.match-indicator')?.textContent).toBe('1 of 1'));
            const currentMark = container.querySelector('mark.search-match-current')!;
            const scrollContainer = container.querySelector(file_type === 'json' ? '.json-fold-renderer .code-content' : '.markdown-content') as HTMLElement;
            const reveal = vi.spyOn(Element.prototype, 'scrollIntoView').mockImplementation(function (this: Element): void {
                expect(this === currentMark || this.contains(currentMark)).toBe(true);
                scrollContainer.scrollTop = 0;
            });
            try {
                for (let repeat = 0; repeat < 2; repeat++) {
                    scrollContainer.scrollTop = 500;
                    await fireEvent.scroll(scrollContainer);
                    reveal.mockClear();
                    if (navigation === 'next button' || navigation === 'previous button') {
                        await fireEvent.click(container.querySelector(navigation === 'next button' ? '.search-next' : '.search-previous')!);
                    } else {
                        await fireEvent.keyDown(window, { key: 'Enter', shiftKey: navigation === 'Shift+Enter' });
                    }
                    await vi.waitFor(() => expect(reveal).toHaveBeenCalledOnce());
                    expect(scrollContainer.scrollTop).toBe(0);
                    expect(container.querySelector('.match-indicator')?.textContent).toBe('1 of 1');
                    expect(container.querySelector('mark.search-match-current')).toBe(currentMark);
                }
            } finally {
                reveal.mockRestore();
            }
        });
    });
    it.each([
        { file_type: 'text' as const, matchedLine: 'needle needle' },
        { file_type: 'code' as const, matchedLine: 'needle needle' },
        { file_type: 'text' as const, matchedLine: 'needle' },
        { file_type: 'code' as const, matchedLine: 'needle' },
    ])('$file_type repeats same-line navigation for "$matchedLine" after manual scrolling and query reopening', async ({ file_type, matchedLine }) => {
        const content = Array.from({ length: 1000 }, (_, index) => index === 900 ? matchedLine : `line ${index}`).join('\n');
        const { container } = await mountFile({ content, file_type });
        await fireEvent.keyDown(window, { key: 'f', ctrlKey: true });
        await fireEvent.input(container.querySelector('input')!, { target: { value: 'needle' } });
        const scroller = container.querySelector('.virtual-scroller') as HTMLDivElement;
        expect(scroller.scrollTop).toBe(900 * 21);
        scroller.scrollTop = 0;
        await fireEvent.scroll(scroller);
        expect(container.querySelector('mark.search-match-current')).toBeNull();
        await fireEvent.keyDown(window, { key: 'Enter' });
        expect(scroller.scrollTop).toBe(900 * 21);
        expect(container.querySelector('mark.search-match-current')?.textContent).toBe('needle');
        await fireEvent.keyDown(window, { key: 'Escape' });
        scroller.scrollTop = 0;
        await fireEvent.scroll(scroller);
        await fireEvent.keyDown(window, { key: 'f', ctrlKey: true });
        await fireEvent.input(container.querySelector('input')!, { target: { value: 'needle' } });
        expect(scroller.scrollTop).toBe(900 * 21);
    });
    it('registers the keydown listener and Escape calls view.hide', async () => {
        await mountFile({ content: 'hello', file_type: 'text' });
        await fireEvent.keyDown(window, { key: 'Escape' });
        expect(callMock).toHaveBeenCalledWith('view.hide', expect.anything());
    });

    it('Ctrl+F opens the search bar and focuses its input', async () => {
        const { container } = await mountFile({ content: 'find the needle here', file_type: 'text' });
        await fireEvent.keyDown(window, { key: 'f', ctrlKey: true });

        const input = container.querySelector('.search-bar input') as HTMLInputElement;
        expect(input).not.toBeNull();
        expect(document.activeElement).toBe(input);
    });

    it('Escape closes the bar before closing the viewer', async () => {
        await mountFile({ content: 'find the needle here', file_type: 'text' });

        await fireEvent.keyDown(window, { key: 'f', ctrlKey: true });
        await fireEvent.keyDown(window, { key: 'Escape' });

        expect(callMock).not.toHaveBeenCalledWith('view.hide', expect.anything());

        await fireEvent.keyDown(window, { key: 'Escape' });
        expect(callMock).toHaveBeenCalledWith('view.hide', expect.anything());
    });

    it('does not intercept a bare Enter when search is closed', async () => {
        await mountFile({ content: 'find the needle here', file_type: 'text' });
        await fireEvent.keyDown(window, { key: 'Enter' });
        expect(callMock).not.toHaveBeenCalledWith('view.hide', expect.anything());
        // No search bar ever opened, so there is nothing to navigate either.
    });

    it('typing a query shows the match count and highlights matches', async () => {
        const { container } = await mountFile({ content: 'cat cat cat', file_type: 'text' });
        await fireEvent.keyDown(window, { key: 'f', ctrlKey: true });

        const input = container.querySelector('.search-bar input') as HTMLInputElement;
        await fireEvent.input(input, { target: { value: 'cat' } });

        await vi.waitFor(() => {
            expect(container.querySelector('.match-indicator')?.textContent).toBe('1 of 3');
        });
        expect(container.querySelectorAll('mark.search-match')).toHaveLength(3);
    });

    it('shows "No matches" for a query with no hits', async () => {
        const { container } = await mountFile({ content: 'cat cat cat', file_type: 'text' });
        await fireEvent.keyDown(window, { key: 'f', ctrlKey: true });

        const input = container.querySelector('.search-bar input') as HTMLInputElement;
        await fireEvent.input(input, { target: { value: 'zzz' } });

        await vi.waitFor(() => {
            expect(container.querySelector('.match-indicator')?.textContent).toBe('No matches');
        });
        expect(container.querySelectorAll('mark.search-match')).toHaveLength(0);
    });

    it('an empty query shows no indicator and no marks', async () => {
        const { container } = await mountFile({ content: 'cat cat cat', file_type: 'text' });
        await fireEvent.keyDown(window, { key: 'f', ctrlKey: true });

        const input = container.querySelector('.search-bar input') as HTMLInputElement;
        await fireEvent.input(input, { target: { value: 'cat' } });
        await vi.waitFor(() => {
            expect(container.querySelectorAll('mark.search-match')).toHaveLength(3);
        });

        await fireEvent.input(input, { target: { value: '' } });
        await vi.waitFor(() => {
            expect(container.querySelectorAll('mark.search-match')).toHaveLength(0);
        });
        expect(container.querySelector('.match-indicator')?.textContent).toBe('');
    });

    it.each(['text', 'markdown'] as const)('%s Enter and Shift+Enter navigate forward and backward, wrapping at both ends', async (file_type) => {
        const { container } = await mountFile({ content: 'cat cat cat', file_type });
        await fireEvent.keyDown(window, { key: 'f', ctrlKey: true });

        const input = container.querySelector('.search-bar input') as HTMLInputElement;
        await fireEvent.input(input, { target: { value: 'cat' } });
        await vi.waitFor(() => {
            expect(container.querySelector('.match-indicator')?.textContent).toBe('1 of 3');
        });

        await fireEvent.keyDown(window, { key: 'Enter' });
        expect(container.querySelector('.match-indicator')?.textContent).toBe('2 of 3');

        await fireEvent.keyDown(window, { key: 'Enter' });
        expect(container.querySelector('.match-indicator')?.textContent).toBe('3 of 3');

        // Forward wrap: past the last match returns to the first.
        await fireEvent.keyDown(window, { key: 'Enter' });
        expect(container.querySelector('.match-indicator')?.textContent).toBe('1 of 3');

        // Backward wrap: before the first match returns to the last.
        await fireEvent.keyDown(window, { key: 'Enter', shiftKey: true });
        expect(container.querySelector('.match-indicator')?.textContent).toBe('3 of 3');
        expect(container.querySelectorAll('mark.search-match')[2].classList.contains('search-match-current')).toBe(true);
    });

    it('closing search via the close button restores the viewer to its unsearched state', async () => {
        const { container } = await mountFile({ content: 'cat cat cat', file_type: 'text' });
        await fireEvent.keyDown(window, { key: 'f', ctrlKey: true });

        const input = container.querySelector('.search-bar input') as HTMLInputElement;
        await fireEvent.input(input, { target: { value: 'cat' } });
        await vi.waitFor(() => {
            expect(container.querySelectorAll('mark.search-match')).toHaveLength(3);
        });

        await fireEvent.click(container.querySelector('.search-close') as HTMLButtonElement);

        expect(container.querySelector('.search-bar')).toBeNull();
        expect(container.querySelectorAll('mark.search-match')).toHaveLength(0);
    });

    it('pressing Escape while the search input itself has focus closes the bar without throwing', async () => {
        const { container } = await mountFile({ content: 'cat cat cat', file_type: 'text' });
        await fireEvent.keyDown(window, { key: 'f', ctrlKey: true });

        const input = container.querySelector('.search-bar input') as HTMLInputElement;
        expect(document.activeElement).toBe(input);

        await fireEvent.keyDown(window, { key: 'Escape' });

        expect(container.querySelector('.search-bar')).toBeNull();
        expect(callMock).not.toHaveBeenCalledWith('view.hide', expect.anything());
    });

    it('repeated mount/unmount cycles do not leak keydown listeners across instances', async () => {
        const first = await mountFile({ content: 'first file', file_type: 'text' });
        first.unmount();
        callMock.mockReset();

        await mountFile({ content: 'second file', file_type: 'text' });
        await fireEvent.keyDown(window, { key: 'Escape' });

        // Exactly one view.hide call — if the first instance's listener had
        // leaked, this would be called twice (once per lingering instance).
        const hideCalls = callMock.mock.calls.filter(([method]) => method === 'view.hide');
        expect(hideCalls).toHaveLength(1);
    });

    it('a match beyond the virtualization window (over 500 lines) is revealed and highlighted', async () => {
        const lines = Array.from({ length: 600 }, (_, i) => (i === 550 ? 'the needle is here' : `line number ${i}`));
        const { container } = await mountFile({ content: lines.join('\n'), file_type: 'text' });
        await fireEvent.keyDown(window, { key: 'f', ctrlKey: true });

        const input = container.querySelector('.search-bar input') as HTMLInputElement;
        await fireEvent.input(input, { target: { value: 'needle' } });

        await vi.waitFor(() => {
            expect(container.querySelector('mark.search-match')).not.toBeNull();
        });
        expect(container.querySelector('mark.search-match')?.textContent).toBe('needle');
    });

    it.each([
        { file_type: 'json' as const, content: '{"a":{"b":{"value":"needle"}},"sibling":{"value":"other"}}', collapseLines: ['3', '7', '2', '1'], hiddenSibling: '8' },
        { file_type: 'code' as const, content: 'function outer() {\n  const a = 1;\n  function inner() {\n    return needle;\n  }\n  return a;\n}\nfunction sibling() {\n  return other;\n}', collapseLines: ['3', '8', '1'], hiddenSibling: '9' },
    ])('$file_type collapsed nested folds reveal only match ancestors and stay expanded after closing search', async ({ file_type, content, collapseLines, hiddenSibling }) => {
        const { container } = await mountFile({
            content,
            file_type,
        });
        for (const lineNumber of collapseLines) {
            const gutterLine = Array.from(container.querySelectorAll('.gutter-line')).find((line) => line.querySelector('.line-number')?.textContent === lineNumber)!;
            await fireEvent.click(gutterLine.querySelector('button')!);
        }
        expect(container.querySelector('[data-line="4"]')).toBeNull();
        expect(container.querySelector(`[data-line="${hiddenSibling}"]`)).toBeNull();
        await fireEvent.keyDown(window, { key: 'f', ctrlKey: true });

        const input = container.querySelector('.search-bar input') as HTMLInputElement;
        await fireEvent.input(input, { target: { value: 'needle' } });

        await vi.waitFor(() => {
            expect(container.querySelector('mark.search-match')).not.toBeNull();
        });
        expect(container.querySelector('mark.search-match')?.textContent).toBe('needle');
        expect(container.querySelector(`[data-line="${hiddenSibling}"]`)).toBeNull();
        expect(container.querySelectorAll('.fold-marker.collapsed')).toHaveLength(1);
        await fireEvent.keyDown(window, { key: 'Escape' });
        expect(container.querySelector('[data-line="4"]')).not.toBeNull();
        expect(container.querySelector(`[data-line="${hiddenSibling}"]`)).toBeNull();
    });

    it('Markdown search finds rendered prose but never a mermaid diagram source', async () => {
        const content = '# Heading\n\n```mermaid\ngraph TD;\n  needle --> B;\n```\n\nFind the needle in prose.';
        const { container } = await mountFile({ content, file_type: 'markdown' });
        await vi.waitFor(() => expect(container.querySelector('.diagram-block svg')?.textContent).toBe('needle diagram'));
        await fireEvent.keyDown(window, { key: 'f', ctrlKey: true });

        const input = container.querySelector('.search-bar input') as HTMLInputElement;
        await fireEvent.input(input, { target: { value: 'needle' } });

        await vi.waitFor(() => {
            expect(container.querySelectorAll('mark.search-match')).toHaveLength(1);
        });
        expect(container.querySelector('.diagram-block mark.search-match')).toBeNull();
    });
});
