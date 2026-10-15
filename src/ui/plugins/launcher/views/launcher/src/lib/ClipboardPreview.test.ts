import { describe, expect, it } from 'vitest';
import { render } from '@testing-library/svelte/svelte5';
import ClipboardPreview from './ClipboardPreview.svelte';

describe('ClipboardPreview.svelte', () => {
  it('shows no truncation line when the whole text is shown', () => {
    const { container } = render(ClipboardPreview, {
      preview: { kind: 'text', text: 'short', hiddenCharacters: 0, details: '1m ago' },
    });
    expect(container.querySelector('.clipboard-preview-text')?.textContent).toBe('short');
    expect(container.querySelector('.clipboard-preview-truncated')).toBeNull();
  });

  it('renders an image entry from its data URI with a text alternative', () => {
    const source = 'data:image/png;base64,AAAA';
    const { container } = render(ClipboardPreview, {
      preview: { kind: 'image', source, details: '1h ago · Image 1280x720' },
    });
    const image = container.querySelector('img.clipboard-preview-image') as HTMLImageElement;
    expect(image).not.toBeNull();
    expect(image.getAttribute('src')).toBe(source);
    // Not "Clipboard image": screen readers already announce the image role,
    // and Svelte flags the repetition (a11y_img_redundant_alt).
    expect(image.getAttribute('alt')).toBe('Clipboard entry');
    expect(container.querySelector('.clipboard-preview-text')).toBeNull();
  });

  it('starts a newly shown entry at its top', async () => {
    const { container, rerender } = render(ClipboardPreview, {
      preview: { kind: 'text', text: 'first', hiddenCharacters: 0, details: '1m ago' },
    });
    const body = container.querySelector('.clipboard-preview-body') as HTMLElement;
    body.scrollTop = 120;
    await rerender({
      preview: { kind: 'text', text: 'second', hiddenCharacters: 0, details: '2m ago' },
    });
    expect(container.querySelector('.clipboard-preview-text')?.textContent).toBe('second');
    expect(body.scrollTop).toBe(0);
  });

  it('says how many characters are not shown for a truncated text', () => {
    const { container } = render(ClipboardPreview, {
      preview: { kind: 'text', text: 'start', hiddenCharacters: 250000, details: '1m ago' },
    });
    expect(container.querySelector('.clipboard-preview-truncated')?.textContent).toBe(
      `\u2026 ${(250000).toLocaleString()} more characters not shown`
    );
  });
});
