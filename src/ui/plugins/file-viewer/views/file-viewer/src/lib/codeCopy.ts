// Header strip and copy button for fenced code blocks in rendered markdown.
// Look settled in docs/playgrounds/2026-10-31-markdown-code-copy-button.html
// ("Header strip"). The page is already a secure context (live probe,
// 2026-10-31), so navigator.clipboard.writeText works from a click.
import { escapeHtml } from './highlighter';

export const COPY_FEEDBACK_MILLISECONDS = 1500;

type CopyState = 'idle' | 'copied' | 'failed';

const ICON_COPY = '<svg class="code-copy-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="9" y="9" width="11" height="11" rx="2"></rect><path d="M5 15V6a2 2 0 0 1 2-2h9"></path></svg>';
const ICON_CHECK = '<svg class="code-copy-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 12.5l4.5 4.5L19 7.5"></path></svg>';
const ICON_ALERT = '<svg class="code-copy-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" aria-hidden="true"><path d="M12 7v6"></path><path d="M12 17h.01"></path><circle cx="12" cy="12" r="9"></circle></svg>';

const CONTENT: Record<CopyState, string> = {
    idle: `${ICON_COPY}<span class="code-copy-label">Copy</span>`,
    copied: `${ICON_CHECK}<span class="code-copy-label">Copied</span>`,
    failed: `${ICON_ALERT}<span class="code-copy-label">Copy failed</span>`,
};

const resetTimers = new WeakMap<HTMLButtonElement, ReturnType<typeof setTimeout>>();

export function codeHeaderHtml(language: string | undefined): string {
    return `<div class="code-header"><span class="code-language">${escapeHtml(language ?? '')}</span>` +
        `<button type="button" class="code-copy" data-copy-state="idle" aria-label="Copy code" title="Copy code">${CONTENT.idle}</button></div>`;
}

function showState(button: HTMLButtonElement, state: CopyState): void {
    button.dataset.copyState = state;
    button.innerHTML = CONTENT[state];
}

async function writeToClipboard(text: string): Promise<boolean> {
    if (typeof navigator.clipboard?.writeText !== 'function') return false;
    try {
        await navigator.clipboard.writeText(text);
        return true;
    } catch (error) {
        console.error('Copying code failed:', error);
        return false;
    }
}

export function handleCodeCopyClick(event: Event): void {
    const target = event.target;
    if (!(target instanceof Element)) return;
    const button = target.closest<HTMLButtonElement>('button.code-copy');
    if (!button) return;
    const code = button.closest('.code-block')?.querySelector('pre code');
    if (!code) return;

    const idleWidth = button.getBoundingClientRect().width;
    void writeToClipboard(code.textContent ?? '').then((copied) => {
        clearTimeout(resetTimers.get(button));
        showState(button, copied ? 'copied' : 'failed');
        // Never shrink while feedback shows; grow only if the longer label needs it.
        button.style.minWidth = `${Math.max(idleWidth, button.getBoundingClientRect().width)}px`;
        resetTimers.set(button, setTimeout(() => {
            showState(button, 'idle');
            button.style.minWidth = '';
        }, COPY_FEEDBACK_MILLISECONDS));
    });
}
