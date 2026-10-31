import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest';
import { attachCodeHeaders, createCodeCopyClickHandler, CODE_COPY_RESERVED_CLASSES, COPY_FEEDBACK_MILLISECONDS } from './codeCopy';

const NONCE = 'nonce-under-test';
let listener: ((event: Event) => void) | undefined;

// Builds a fenced-code fixture the way the code renderer does, then runs the header pass on it.
function mountBlock(code: string, language?: string, buttonNonce: string = NONCE): HTMLButtonElement {
    const root = document.createElement('div');
    const pre = document.createElement('pre');
    pre.dataset.copyNonce = buttonNonce;
    if (language !== undefined) pre.dataset.codeLanguage = language;
    const codeElement = document.createElement('code');
    codeElement.className = 'hljs';
    codeElement.textContent = code;
    pre.append(codeElement);
    root.append(pre);
    document.body.innerHTML = '';
    document.body.append(root);
    attachCodeHeaders(root, buttonNonce);
    listener = createCodeCopyClickHandler(() => NONCE);
    document.body.addEventListener('click', listener);
    return document.querySelector('button.code-copy')!;
}

function statusText(): string | null | undefined {
    return document.querySelector('.code-copy-status')?.textContent;
}

function stubClipboard(writeText: (text: string) => Promise<void>): void {
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
}

// Flushes the copy promise chain without racing the feedback timers.
async function settle(): Promise<void> {
    await vi.advanceTimersByTimeAsync(0);
}

beforeEach(() => vi.useFakeTimers());
afterEach(() => {
    if (listener) document.body.removeEventListener('click', listener);
    listener = undefined;
    document.body.innerHTML = '';
    vi.useRealTimers();
    Object.defineProperty(navigator, 'clipboard', { value: undefined, configurable: true });
});

describe('CODE_COPY_RESERVED_CLASSES', () => {
    it('names every class the header and its copy button use', () => {
        expect([...CODE_COPY_RESERVED_CLASSES].sort()).toEqual([
            'code-block',
            'code-copy',
            'code-copy-icon',
            'code-copy-label',
            'code-copy-status',
            'code-header',
            'code-language',
        ]);
    });
});

describe('attachCodeHeaders', () => {
    function fixture(html: string): HTMLElement {
        const root = document.createElement('div');
        root.innerHTML = html;
        document.body.innerHTML = '';
        document.body.append(root);
        return root;
    }

    it('wraps only pre elements carrying the nonce, and wraps each one once', () => {
        const root = fixture('<pre data-copy-nonce="n" data-code-language="ts"><code>a</code></pre><pre data-copy-nonce="other"><code>b</code></pre>');
        attachCodeHeaders(root, 'n');
        attachCodeHeaders(root, 'n');
        expect(root.querySelectorAll('.code-header')).toHaveLength(1);
        expect(root.querySelector('.code-block pre')?.textContent).toBe('a');
        const untouched = root.querySelector<HTMLPreElement>('pre[data-copy-nonce="other"]')!;
        expect(untouched.parentElement).toBe(root);
    });

    it('builds the header with the language as text and the same button the copy handler expects', () => {
        const root = fixture('<pre data-copy-nonce="n" data-code-language="&lt;b&gt;x"><code>a</code></pre>');
        attachCodeHeaders(root, 'n');
        expect(root.querySelector('.code-language')?.textContent).toBe('<b>x');
        expect(root.querySelector('.code-language b')).toBeNull();
        const button = root.querySelector<HTMLButtonElement>('button.code-copy')!;
        expect(button.getAttribute('type')).toBe('button');
        expect(button.dataset.copyState).toBe('idle');
        expect(button.dataset.copyNonce).toBe('n');
        expect(button.getAttribute('aria-label')).toBe('Copy code');
        expect(button.getAttribute('title')).toBe('Copy code');
        expect(button.textContent).toBe('Copy');
        const status = root.querySelector('.code-header .code-copy-status')!;
        expect(status.getAttribute('role')).toBe('status');
        expect(status.getAttribute('aria-live')).toBe('polite');
        expect(status.textContent).toBe('');
    });

    it('leaves the language empty when the fence names none', () => {
        const root = fixture('<pre data-copy-nonce="n"><code>a</code></pre>');
        attachCodeHeaders(root, 'n');
        expect(root.querySelector('.code-language')?.textContent).toBe('');
    });
});

describe('header built by attachCodeHeaders', () => {
    it('renders the language and an accessible copy button', () => {
        const button = mountBlock('x', 'rust');
        expect(document.querySelector('.code-language')?.textContent).toBe('rust');
        expect(button.getAttribute('type')).toBe('button');
        expect(button.getAttribute('aria-label')).toBe('Copy code');
        expect(button.getAttribute('title')).toBe('Copy code');
        expect(button.textContent).toBe('Copy');
        expect(button.dataset.copyState).toBe('idle');
        expect(button.dataset.copyNonce).toBe(NONCE);
    });

    it('keeps a hostile nonce as an attribute value, never as markup', () => {
        const button = mountBlock('x', undefined, '"><b>x</b>');
        expect(document.querySelector('b')).toBeNull();
        expect(button.dataset.copyNonce).toBe('"><b>x</b>');
    });
});

describe('createCodeCopyClickHandler', () => {
    it('copies exactly the block text and shows Copied, then resets', async () => {
        const writeText = vi.fn(async () => {});
        stubClipboard(writeText);
        const button = mountBlock('  const s = "<b>&amp;";\n  return s;');
        button.click();
        await settle();
        expect(button.dataset.copyState).toBe('copied');
        expect(writeText).toHaveBeenCalledWith('  const s = "<b>&amp;";\n  return s;');
        expect(button.textContent).toBe('Copied');
        expect(statusText()).toBe('Copied');
        vi.advanceTimersByTime(COPY_FEEDBACK_MILLISECONDS);
        expect(button.dataset.copyState).toBe('idle');
        expect(button.textContent).toBe('Copy');
        expect(statusText()).toBe('');
    });

    it('shows Copy failed when the write rejects, then resets', async () => {
        stubClipboard(async () => { throw new Error('denied'); });
        const button = mountBlock('x');
        button.click();
        await settle();
        expect(button.dataset.copyState).toBe('failed');
        expect(button.textContent).toBe('Copy failed');
        expect(statusText()).toBe('Copy failed');
        vi.advanceTimersByTime(COPY_FEEDBACK_MILLISECONDS);
        expect(button.dataset.copyState).toBe('idle');
        expect(statusText()).toBe('');
    });

    it('shows Copy failed when the clipboard API is missing', async () => {
        const button = mountBlock('x');
        button.click();
        await settle();
        expect(button.dataset.copyState).toBe('failed');
    });

    it('restarts the feedback timer on a second click instead of stacking', async () => {
        stubClipboard(async () => {});
        const button = mountBlock('x');
        button.click();
        await settle();
        expect(button.dataset.copyState).toBe('copied');
        vi.advanceTimersByTime(COPY_FEEDBACK_MILLISECONDS - 100);
        button.click();
        await settle();
        expect(button.dataset.copyState).toBe('copied');
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

    it('ignores a copy button whose nonce is not the current render nonce', async () => {
        const writeText = vi.fn(async () => {});
        stubClipboard(writeText);
        const button = mountBlock('x', 'sh', 'forged-nonce');
        button.click();
        await settle();
        expect(writeText).not.toHaveBeenCalled();
        expect(button.dataset.copyState).toBe('idle');
        expect(button.textContent).toBe('Copy');
        expect(statusText()).toBe('');
    });

    it('ignores a completion from an older click once a newer click has started', async () => {
        const pending: Array<{ resolve: () => void; reject: (error: Error) => void }> = [];
        const writeText = vi.fn(() => new Promise<void>((resolve, reject) => {
            pending.push({ resolve, reject });
        }));
        stubClipboard(writeText);
        const button = mountBlock('x');
        button.click();
        button.click();
        expect(pending).toHaveLength(2);
        pending[1].resolve();
        await settle();
        expect(button.dataset.copyState).toBe('copied');
        pending[0].reject(new Error('late failure'));
        await settle();
        expect(button.dataset.copyState).toBe('copied');
        expect(button.textContent).toBe('Copied');
        expect(statusText()).toBe('Copied');
    });

    it('holds the idle width as a minimum while feedback shows, then releases it', async () => {
        stubClipboard(async () => {});
        const button = mountBlock('x');
        button.getBoundingClientRect = () => ({ width: 60 }) as DOMRect;
        button.click();
        await settle();
        expect(button.style.minWidth).toBe('60px');
        vi.advanceTimersByTime(COPY_FEEDBACK_MILLISECONDS);
        expect(button.style.minWidth).toBe('');
    });

    it('sets no minimum width when the idle width measures zero', async () => {
        stubClipboard(async () => {});
        const button = mountBlock('x');
        button.getBoundingClientRect = () => ({ width: 0 }) as DOMRect;
        button.click();
        await settle();
        expect(button.dataset.copyState).toBe('copied');
        expect(button.style.minWidth).toBe('');
    });
});
