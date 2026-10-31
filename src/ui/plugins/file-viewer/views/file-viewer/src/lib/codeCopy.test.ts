import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest';
import { codeHeaderHtml, handleCodeCopyClick, COPY_FEEDBACK_MILLISECONDS } from './codeCopy';

function mountBlock(code: string, language?: string): HTMLButtonElement {
    document.body.innerHTML = `<div class="code-block">${codeHeaderHtml(language)}<pre><code class="hljs"></code></pre></div>`;
    document.querySelector('code')!.textContent = code;
    document.body.addEventListener('click', handleCodeCopyClick);
    return document.querySelector('button.code-copy')!;
}

function stubClipboard(writeText: (text: string) => Promise<void>): void {
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
}

beforeEach(() => vi.useFakeTimers());
afterEach(() => {
    document.body.removeEventListener('click', handleCodeCopyClick);
    document.body.innerHTML = '';
    vi.useRealTimers();
    Object.defineProperty(navigator, 'clipboard', { value: undefined, configurable: true });
});

describe('codeHeaderHtml', () => {
    it('renders the language and an accessible copy button', () => {
        const button = mountBlock('x', 'rust');
        expect(document.querySelector('.code-language')?.textContent).toBe('rust');
        expect(button.getAttribute('type')).toBe('button');
        expect(button.getAttribute('aria-label')).toBe('Copy code');
        expect(button.getAttribute('title')).toBe('Copy code');
        expect(button.textContent).toBe('Copy');
        expect(button.dataset.copyState).toBe('idle');
    });

    it('escapes a hostile language name', () => {
        mountBlock('x', '<img src=x onerror=alert(1)>');
        expect(document.querySelector('.code-language img')).toBeNull();
        expect(document.querySelector('.code-language')?.textContent).toBe('<img src=x onerror=alert(1)>');
    });

    it('leaves the language empty when the fence names none', () => {
        mountBlock('x');
        expect(document.querySelector('.code-language')?.textContent).toBe('');
    });
});

describe('handleCodeCopyClick', () => {
    it('copies exactly the block text and shows Copied, then resets', async () => {
        const writeText = vi.fn(async () => {});
        stubClipboard(writeText);
        const button = mountBlock('  const s = "<b>&amp;";\n  return s;');
        button.click();
        await vi.waitFor(() => expect(button.dataset.copyState).toBe('copied'));
        expect(writeText).toHaveBeenCalledWith('  const s = "<b>&amp;";\n  return s;');
        expect(button.textContent).toBe('Copied');
        vi.advanceTimersByTime(COPY_FEEDBACK_MILLISECONDS);
        expect(button.dataset.copyState).toBe('idle');
        expect(button.textContent).toBe('Copy');
    });

    it('shows Copy failed when the write rejects', async () => {
        stubClipboard(async () => { throw new Error('denied'); });
        const button = mountBlock('x');
        button.click();
        await vi.waitFor(() => expect(button.dataset.copyState).toBe('failed'));
        expect(button.textContent).toBe('Copy failed');
        vi.advanceTimersByTime(COPY_FEEDBACK_MILLISECONDS);
        expect(button.dataset.copyState).toBe('idle');
    });

    it('shows Copy failed when the clipboard API is missing', async () => {
        const button = mountBlock('x');
        button.click();
        await vi.waitFor(() => expect(button.dataset.copyState).toBe('failed'));
    });

    it('restarts the feedback timer on a second click instead of stacking', async () => {
        stubClipboard(async () => {});
        const button = mountBlock('x');
        button.click();
        await vi.waitFor(() => expect(button.dataset.copyState).toBe('copied'));
        vi.advanceTimersByTime(COPY_FEEDBACK_MILLISECONDS - 100);
        button.click();
        await vi.waitFor(() => expect(button.dataset.copyState).toBe('copied'));
        vi.advanceTimersByTime(200);
        expect(button.dataset.copyState).toBe('copied');
        vi.advanceTimersByTime(COPY_FEEDBACK_MILLISECONDS);
        expect(button.dataset.copyState).toBe('idle');
    });

    it('ignores clicks that are not on a copy button', () => {
        const writeText = vi.fn(async () => {});
        stubClipboard(writeText);
        mountBlock('x');
        document.querySelector<HTMLElement>('code')!.click();
        expect(writeText).not.toHaveBeenCalled();
    });
});
