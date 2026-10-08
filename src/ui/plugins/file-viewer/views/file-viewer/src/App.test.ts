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
    file_type: 'text' | 'code' | 'json' | 'markdown' | 'image' | 'video';
    language?: string | null;
    uri?: string | null;
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
                uri: fixture.uri ?? null,
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

describe('App overview ruler', () => {
    it.each(['markdown', 'json', 'code', 'text'] as const)('reserves the overview ruler strip for %s, even with no active search', async (file_type) => {
        const content = file_type === 'json' ? '{"a":1}' : 'hello world';
        const { container } = await mountFile({ content, file_type });
        expect(container.querySelector('.overview-ruler')).not.toBeNull();
    });

    it.each(['image', 'video'] as const)('renders no overview ruler for %s', async (file_type) => {
        const { container } = await mountFile({ content: '', file_type, uri: 'quantum://fixture/placeholder' });
        expect(container.querySelector('.overview-ruler')).toBeNull();
    });

    it.each([
        { file_type: 'image' as const, rootSelector: '.image-renderer' },
        { file_type: 'video' as const, rootSelector: '.container' },
    ])('keeps the $file_type renderer full width by wrapping it in a renderer pane', async ({ file_type, rootSelector }) => {
        const { container } = await mountFile({ content: '', file_type, uri: 'quantum://fixture/placeholder' });
        const root = container.querySelector(rootSelector);
        expect(root).not.toBeNull();
        expect(root!.parentElement?.classList.contains('renderer-pane')).toBe(true);
    });

    it('clicking a match mark on the ruler navigates to it and scrolls it into view', async () => {
        // Code's non-virtual (short-file) match positions are pure row
        // arithmetic (no async DOM measurement), so the ruler mark for each
        // match is available deterministically, without waiting on a frame.
        // The track-height stub must be in place BEFORE the ruler's own
        // mount effect runs its first measurement — OverviewRuler only
        // re-measures on a later scroll/resize event, so stubbing
        // afterward would leave it holding a stale trackHeight of 0.
        const trackHeight = 600;
        const heightSpy = vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockReturnValue(trackHeight);
        const rectSpy = vi
            .spyOn(HTMLElement.prototype, 'getBoundingClientRect')
            .mockReturnValue({ top: 0, left: 0, right: 10, bottom: trackHeight, width: 10, height: trackHeight, x: 0, y: 0, toJSON() { return this; } } as DOMRect);
        const reveal = vi.spyOn(Element.prototype, 'scrollIntoView');
        try {
            const content = 'cat\ndog\ncat\nbird\ncat';
            const { container } = await mountFile({ content, file_type: 'code' });
            await fireEvent.keyDown(window, { key: 'f', ctrlKey: true });
            await fireEvent.input(container.querySelector('.search-bar input')!, { target: { value: 'cat' } });
            await vi.waitFor(() => expect(container.querySelector('.match-indicator')?.textContent).toBe('1 of 3'));

            const ruler = container.querySelector('.overview-ruler') as HTMLElement;
            await vi.waitFor(() => expect(ruler.querySelectorAll('.ruler-mark').length).toBeGreaterThan(0));

            // The third match ("cat" on the fifth, last line) sits near the
            // bottom of a five-row, no-fold file: row 4 of 5, padding 12,
            // row height 20.8.
            const fraction = (12 + 4 * 20.8 + 20.8 / 2) / (12 + 5 * 20.8 + 12);
            const clickY = Math.round(fraction * trackHeight);

            await fireEvent.click(ruler, { clientY: clickY });
            await vi.waitFor(() => expect(container.querySelector('.match-indicator')?.textContent).toBe('3 of 3'));
            await vi.waitFor(() => expect(reveal).toHaveBeenCalled());
            const revealedElement = reveal.mock.contexts.at(-1) as unknown as Element;
            expect(revealedElement.getAttribute('data-line')).toBe('5');
        } finally {
            heightSpy.mockRestore();
            rectSpy.mockRestore();
            reveal.mockRestore();
        }
    });

    it('closing search removes the ruler marks but keeps the reserved strip', async () => {
        // Code's non-virtual match positions are pure row arithmetic, so a
        // mark appears deterministically without stubbing live DOM layout
        // (jsdom has no layout engine, so a wrapping-text position —
        // measured live — would always read back a zero-height rect here).
        const { container } = await mountFile({ content: 'cat cat cat', file_type: 'code' });
        await fireEvent.keyDown(window, { key: 'f', ctrlKey: true });
        await fireEvent.input(container.querySelector('.search-bar input')!, { target: { value: 'cat' } });
        await vi.waitFor(() => expect(container.querySelector('.match-indicator')?.textContent).toBe('1 of 3'));
        await vi.waitFor(() => expect(container.querySelectorAll('.ruler-mark').length).toBeGreaterThan(0));

        await fireEvent.keyDown(window, { key: 'Escape' });

        expect(container.querySelector('.overview-ruler')).not.toBeNull();
        expect(container.querySelectorAll('.ruler-mark')).toHaveLength(0);
    });
});

describe('App routing', () => {
    it('routes {path} to the existing file viewer, unchanged', async () => {
        const { container } = await mountFile({ content: 'hello', file_type: 'text' });
        expect(container.querySelector('.diff-view')).toBeNull();
        expect(container.querySelector('header')).not.toBeNull();
    });

    it('routes {diff} args to DiffView with a git source, defaulting base to HEAD and target to null', async () => {
        (window as any).__quantum_args = { diff: { repository: '/repository' } };
        callMock.mockImplementation((method: string) => {
            if (method === 'file-viewer.changes') {
                return Promise.resolve({ repository_root: '/repository', base_label: 'HEAD', target_label: 'working tree', stageable: true, files: [] });
            }
            return Promise.resolve(undefined);
        });
        const { container } = render(App);
        await vi.waitFor(() => {
            expect(callMock).toHaveBeenCalledWith('file-viewer.changes', { repository: '/repository', base: 'HEAD', target: null });
        });
        await vi.waitFor(() => {
            expect(container.querySelector('.diff-view')).not.toBeNull();
        });
        // App never reads a file nor registers its own path-mode keydown
        // listener for a diff source.
        expect(callMock).not.toHaveBeenCalledWith('file-viewer.read', expect.anything());
    });

    it('routes {compare} args to DiffView with a pair source', async () => {
        (window as any).__quantum_args = { compare: { left: '/a.ts', right: '/b.ts' } };
        callMock.mockImplementation((method: string, params: any) => {
            if (method === 'file-viewer.read') {
                return Promise.resolve({
                    content: params.path === '/a.ts' ? 'left' : 'right',
                    file_type: 'code',
                    filename: params.path,
                    directory: '/',
                    size: 4,
                    language: 'typescript',
                    mime_type: null,
                    uri: null,
                });
            }
            return Promise.resolve(undefined);
        });
        const { container } = render(App);
        await vi.waitFor(() => {
            expect(callMock).toHaveBeenCalledWith('file-viewer.read', { path: '/a.ts' });
            expect(callMock).toHaveBeenCalledWith('file-viewer.read', { path: '/b.ts' });
        });
        await vi.waitFor(() => {
            expect(container.querySelector('.diff-view')).not.toBeNull();
        });
    });

    it('"n" while the search input is focused in diff mode types into the input instead of navigating', async () => {
        (window as any).__quantum_args = { diff: { repository: '/repository' } };
        callMock.mockImplementation((method: string) => {
            if (method === 'file-viewer.changes') {
                return Promise.resolve({
                    repository_root: '/repository',
                    base_label: 'HEAD',
                    target_label: 'working tree',
                    stageable: true,
                    files: [
                        {
                            path: 'a.ts',
                            language: 'typescript',
                            base: { content: 'old\n', blob: 'b1', mode: '100644', binary: false, too_large: false },
                            index: { content: 'old\n', blob: 'b1', mode: '100644', binary: false, too_large: false },
                            target: { content: 'new\n', blob: 'b2', mode: '100644', binary: false, too_large: false },
                            untracked: false,
                        },
                    ],
                });
            }
            if (method === 'file-viewer.fingerprint') return Promise.resolve({ fingerprint: 'fingerprint-1' });
            return Promise.resolve(undefined);
        });
        const { container } = render(App);
        await vi.waitFor(() => expect(container.querySelector('.diff-view')).not.toBeNull());

        await fireEvent.keyDown(window, { key: 'f', ctrlKey: true });
        const input = container.querySelector('.search-bar .search-input') as HTMLInputElement;
        expect(input).not.toBeNull();
        input.focus();

        const event = new KeyboardEvent('keydown', { key: 'n', bubbles: true, cancelable: true });
        await fireEvent(input, event);
        expect(event.defaultPrevented).toBe(false);
        expect(document.activeElement).toBe(input);
    });
});
