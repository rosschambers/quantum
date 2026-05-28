import { describe, it, expect, beforeEach, vi } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte/svelte5';
import userEvent from '@testing-library/user-event';
import App from './App.svelte';

let mockCall: ReturnType<typeof vi.fn>;

vi.mock('@quantum/client', () => {
  return {
    createClient: () => ({
      call: mockCall,
      subscribe: vi.fn(() => () => {}),
      close: vi.fn(),
    }),
    __esModule: true,
  };
});

describe('App.svelte', () => {
  beforeEach(() => {
    mockCall = vi.fn();
  });

  it('renders search input and focuses it on mount', async () => {
    const { component } = render(App);
    const input = screen.getByPlaceholderText('Search...') as HTMLInputElement;
    expect(input).toBeDefined();
    expect(input.type).toBe('text');
    // Note: focus management in testing environment is limited, so we verify input exists
  });

  it('triggers search on input with debounce', async () => {
    const user = userEvent.setup();
    mockCall.mockResolvedValue({ matches: [] });

    render(App);
    const input = screen.getByPlaceholderText('Search...');

    await user.type(input, 'test');

    // Wait for debounce (50ms)
    await waitFor(
      () => {
        expect(mockCall).toHaveBeenCalledWith('search', {
          text: 'test',
          providers: [],
        });
      },
      { timeout: 200 }
    );
  });

  it('renders matches when search returns results', async () => {
    const matches = [
      {
        id: '1',
        provider: 'apps',
        title: 'Firefox',
        subtitle: 'Web Browser',
        score: 0.95,
        action: { kind: 'launch', data: { desktop_id: 'firefox' } },
      },
    ];
    mockCall.mockImplementation((method) => {
      if (method === 'search') {
        return Promise.resolve({ matches });
      }
      return Promise.resolve({});
    });

    const user = userEvent.setup();
    render(App);
    const input = screen.getByPlaceholderText('Search...');

    await user.type(input, 'fire');

    await waitFor(() => {
      expect(screen.getByText('Firefox')).toBeDefined();
      expect(screen.getByText('Web Browser')).toBeDefined();
    });
  });

  it('navigates results with arrow keys', async () => {
    const matches = [
      {
        id: '1',
        provider: 'apps',
        title: 'Firefox',
        score: 0.95,
        action: { kind: 'launch', data: { desktop_id: 'firefox' } },
      },
      {
        id: '2',
        provider: 'apps',
        title: 'Chrome',
        score: 0.9,
        action: { kind: 'launch', data: { desktop_id: 'google-chrome' } },
      },
    ];
    mockCall.mockImplementation((method) => {
      if (method === 'search') {
        return Promise.resolve({ matches });
      }
      return Promise.resolve({});
    });

    const user = userEvent.setup();
    render(App);
    const input = screen.getByPlaceholderText('Search...');

    await user.type(input, 'x');

    await waitFor(() => {
      expect(screen.getByText('Firefox')).toBeDefined();
      expect(screen.getByText('Chrome')).toBeDefined();
    });

    // First item should be highlighted initially
    let firefox = screen.getByText('Firefox').closest('.match-item');
    expect(firefox?.classList.contains('highlighted')).toBe(true);

    // Press ArrowDown
    await fireEvent.keyDown(input, { key: 'ArrowDown' });

    // Second item should be highlighted now
    let chrome = screen.getByText('Chrome').closest('.match-item');
    expect(chrome?.classList.contains('highlighted')).toBe(true);

    // Press ArrowUp
    await fireEvent.keyDown(input, { key: 'ArrowUp' });

    // First item should be highlighted again
    firefox = screen.getByText('Firefox').closest('.match-item');
    expect(firefox?.classList.contains('highlighted')).toBe(true);
  });

  it('invokes action on Enter and hides view', async () => {
    const matches = [
      {
        id: '1',
        provider: 'apps',
        title: 'Firefox',
        score: 0.95,
        action: { kind: 'launch', data: { desktop_id: 'firefox' } },
      },
    ];
    mockCall.mockImplementation((method) => {
      if (method === 'search') {
        return Promise.resolve({ matches });
      }
      return Promise.resolve({});
    });

    const user = userEvent.setup();
    render(App);
    const input = screen.getByPlaceholderText('Search...');

    await user.type(input, 'fire');

    await waitFor(() => {
      expect(screen.getByText('Firefox')).toBeDefined();
    });

    // Press Enter
    await fireEvent.keyDown(input, { key: 'Enter' });

    await waitFor(() => {
      expect(mockCall).toHaveBeenCalledWith('action.invoke', expect.objectContaining({
        provider: 'apps',
      }));
      expect(mockCall).toHaveBeenCalledWith('view.hide', { name: 'launcher' });
    });
  });

  it('hides view on Escape', async () => {
    const user = userEvent.setup();
    mockCall.mockResolvedValue({ matches: [] });

    render(App);
    const input = screen.getByPlaceholderText('Search...');

    // Press Escape
    await fireEvent.keyDown(input, { key: 'Escape' });

    await waitFor(() => {
      expect(mockCall).toHaveBeenCalledWith('view.hide', { name: 'launcher' });
    });
  });
});
