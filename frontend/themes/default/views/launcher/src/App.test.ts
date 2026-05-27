import { describe, it, expect, beforeEach, vi } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import App from './App.svelte';

vi.mock('@quantum/client', () => {
  const mockCall = vi.fn();

  return {
    createClient: () => ({
      call: mockCall,
    }),
    __esModule: true,
  };
});

describe('App.svelte', () => {
  let mockClient: any;

  beforeEach(() => {
    vi.clearAllMocks();
    const clientModule = require('@quantum/client');
    mockClient = {
      call: vi.fn(),
    };
    vi.mocked(clientModule.createClient).mockReturnValue(mockClient);
  });

  it('renders search input and focuses it on mount', async () => {
    const { component } = render(App);
    const input = screen.getByPlaceholderText('Search...');
    expect(input).toBeInTheDocument();
    expect(document.activeElement).toBe(input);
  });

  it('triggers search on input with debounce', async () => {
    const user = userEvent.setup();
    mockClient.call.mockResolvedValue({ matches: [] });

    render(App);
    const input = screen.getByPlaceholderText('Search...');

    await user.type(input, 'test');

    // Wait for debounce (50ms)
    await waitFor(
      () => {
        expect(mockClient.call).toHaveBeenCalledWith('search', {
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
    mockClient.call.mockImplementation((method) => {
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
      expect(screen.getByText('Firefox')).toBeInTheDocument();
      expect(screen.getByText('Web Browser')).toBeInTheDocument();
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
    mockClient.call.mockImplementation((method) => {
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
      expect(screen.getByText('Firefox')).toBeInTheDocument();
      expect(screen.getByText('Chrome')).toBeInTheDocument();
    });

    // First item should be highlighted initially
    let firefox = screen.getByText('Firefox').closest('.match-item');
    expect(firefox).toHaveClass('highlighted');

    // Press ArrowDown
    await fireEvent.keyDown(input, { key: 'ArrowDown' });

    // Second item should be highlighted now
    let chrome = screen.getByText('Chrome').closest('.match-item');
    expect(chrome).toHaveClass('highlighted');

    // Press ArrowUp
    await fireEvent.keyDown(input, { key: 'ArrowUp' });

    // First item should be highlighted again
    firefox = screen.getByText('Firefox').closest('.match-item');
    expect(firefox).toHaveClass('highlighted');
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
    mockClient.call.mockImplementation((method) => {
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
      expect(screen.getByText('Firefox')).toBeInTheDocument();
    });

    // Press Enter
    await fireEvent.keyDown(input, { key: 'Enter' });

    await waitFor(() => {
      expect(mockClient.call).toHaveBeenCalledWith('action.invoke', expect.objectContaining({
        provider: 'apps',
      }));
      expect(mockClient.call).toHaveBeenCalledWith('view.hide', { view: 'launcher' });
    });
  });

  it('hides view on Escape', async () => {
    const user = userEvent.setup();
    mockClient.call.mockResolvedValue({ matches: [] });

    render(App);
    const input = screen.getByPlaceholderText('Search...');

    // Press Escape
    await fireEvent.keyDown(input, { key: 'Escape' });

    await waitFor(() => {
      expect(mockClient.call).toHaveBeenCalledWith('view.hide', { view: 'launcher' });
    });
  });
});
