import { afterEach, describe, expect, it } from 'vitest';
import { currentInitialQuery, initialQueryFrom } from './openArgs';

describe('initialQueryFrom', () => {
  it('returns a string query', () => {
    expect(initialQueryFrom({ query: ';' })).toBe(';');
  });
  it('returns empty for null', () => {
    expect(initialQueryFrom(null)).toBe('');
  });
  it('returns empty for undefined', () => {
    expect(initialQueryFrom(undefined)).toBe('');
  });
  it('returns empty for a non-string query', () => {
    expect(initialQueryFrom({ query: 7 })).toBe('');
  });
  it('returns empty when query is missing', () => {
    expect(initialQueryFrom({ path: '/tmp' })).toBe('');
  });
});

describe('currentInitialQuery', () => {
  afterEach(() => {
    delete (window as { __quantum_args?: unknown }).__quantum_args;
  });
  it('reads window.__quantum_args', () => {
    (window as { __quantum_args?: unknown }).__quantum_args = { query: ';' };
    expect(currentInitialQuery()).toBe(';');
  });
});
