import { describe, it, expect } from 'vitest';
import { resolveIcon } from './icon';

describe('resolveIcon', () => {
  it('returns a plain string icon as-is', () => {
    expect(resolveIcon('https://example.com/a.png')).toBe('https://example.com/a.png');
  });

  it('maps a path IconRef to the quantum://icon route, percent-encoded', () => {
    expect(resolveIcon({ kind: 'path', data: '/usr/share/icons/a.png' })).toBe(
      'quantum://icon/%2Fusr%2Fshare%2Ficons%2Fa.png'
    );
  });

  it('returns a data_uri IconRef verbatim', () => {
    const uri = 'data:image/png;base64,iVBORw0KGgo=';
    expect(resolveIcon({ kind: 'data_uri', data: uri })).toBe(uri);
  });

  it('returns undefined for a name IconRef (no loadable URL)', () => {
    expect(resolveIcon({ kind: 'name', data: 'firefox' })).toBeUndefined();
  });

  it('returns undefined for an unknown kind', () => {
    expect(resolveIcon({ kind: 'whatever', data: 'x' })).toBeUndefined();
  });

  it('returns undefined for undefined input', () => {
    expect(resolveIcon(undefined)).toBeUndefined();
  });
});
