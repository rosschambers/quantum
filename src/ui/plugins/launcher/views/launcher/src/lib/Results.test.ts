import { describe, it, expect, vi } from 'vitest';
import { render } from '@testing-library/svelte/svelte5';
import Results from './Results.svelte';
import type { Match } from './types';

function match(overrides: Partial<Match> = {}): Match {
  return {
    id: '1',
    provider: 'test',
    title: 'A result',
    score: 1,
    action: { kind: 'launch', data: {} },
    ...overrides,
  };
}

const dataUri = 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUg==';

describe('Results.svelte', () => {
  it('renders a data_uri icon as a thumbnail image', () => {
    render(Results, {
      items: [match({ icon: { kind: 'data_uri', data: dataUri } })],
      highlighted: 0,
      onSelect: vi.fn(),
    });

    const image = document.querySelector('.match-item img') as HTMLImageElement;
    expect(image).not.toBeNull();
    expect(image.getAttribute('src')).toBe(dataUri);
    // A data_uri is a clipboard thumbnail (a photo-like preview), marked so it
    // can be styled to cover-crop rather than contain like an app glyph.
    expect(image.classList.contains('thumbnail')).toBe(true);
  });

  it('does not render an image for a name icon', () => {
    render(Results, {
      items: [match({ icon: { kind: 'name', data: 'firefox' } })],
      highlighted: 0,
      onSelect: vi.fn(),
    });

    expect(document.querySelector('.match-item img')).toBeNull();
  });

  it('renders a path icon as an image but not a thumbnail', () => {
    render(Results, {
      items: [match({ icon: { kind: 'path', data: '/usr/share/icons/a.png' } })],
      highlighted: 0,
      onSelect: vi.fn(),
    });

    const image = document.querySelector('.match-item img') as HTMLImageElement;
    expect(image).not.toBeNull();
    expect(image.classList.contains('thumbnail')).toBe(false);
  });
});
