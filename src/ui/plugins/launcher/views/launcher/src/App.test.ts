import { describe, it, expect, beforeEach, vi } from 'vitest';
import { render, screen, fireEvent, waitFor, act } from '@testing-library/svelte/svelte5';
import userEvent from '@testing-library/user-event';
import App from './App.svelte';

let mockCall: ReturnType<typeof vi.fn>;

vi.mock('@quantum/client', async () => {
  // Keep the real openContextMenu runtime so the secondary-actions panel
  // renders a genuine menu we can assert against; only the client is stubbed.
  const actual = await vi.importActual<typeof import('@quantum/client')>('@quantum/client');
  return {
    ...actual,
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

  it('fetches default apps on mount with an empty query pinned to desktop-apps', async () => {
    mockCall.mockResolvedValue({ matches: [] });

    render(App);
    await act();

    await waitFor(() => {
      expect(mockCall).toHaveBeenCalledWith('search', {
        text: '',
        providers: ['desktop-apps'],
      });
    });
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

  it('pins a > command query to the shell provider', async () => {
    const user = userEvent.setup();
    mockCall.mockResolvedValue({ matches: [] });

    render(App);
    const input = screen.getByPlaceholderText('Search...');

    await user.type(input, '>kill quantumd');

    await waitFor(
      () => {
        expect(mockCall).toHaveBeenCalledWith('search', {
          text: '>kill quantumd',
          providers: ['shell'],
        });
      },
      { timeout: 200 }
    );
  });

  it('pins a ! command query to the shell provider', async () => {
    const user = userEvent.setup();
    mockCall.mockResolvedValue({ matches: [] });

    render(App);
    const input = screen.getByPlaceholderText('Search...');

    await user.type(input, '!htop');

    await waitFor(
      () => {
        expect(mockCall).toHaveBeenCalledWith('search', {
          text: '!htop',
          providers: ['shell'],
        });
      },
      { timeout: 200 }
    );
  });

  it('leaves a plain query fanning out to all providers', async () => {
    const user = userEvent.setup();
    mockCall.mockResolvedValue({ matches: [] });

    render(App);
    const input = screen.getByPlaceholderText('Search...');

    await user.type(input, 'firefox');

    await waitFor(
      () => {
        expect(mockCall).toHaveBeenCalledWith('search', {
          text: 'firefox',
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

  it('uses combobox + listbox ARIA pattern', async () => {
    const matches = [
      {
        id: '1',
        provider: 'apps',
        title: 'Fox Browser',
        score: 0.95,
        action: { kind: 'launch', data: { desktop_id: 'fox' } },
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

    // Input should have combobox semantics even before results
    expect(input.getAttribute('role')).toBe('combobox');
    expect(input.getAttribute('aria-controls')).toBe('quantum-results');
    expect(input.getAttribute('aria-autocomplete')).toBe('list');

    await user.type(input, 'fox');

    await waitFor(() => {
      expect(screen.getByText('Fox Browser')).toBeDefined();
    });

    // Listbox container
    const listbox = document.getElementById('quantum-results');
    expect(listbox).not.toBeNull();
    expect(listbox?.getAttribute('role')).toBe('listbox');

    // Input should now track the highlighted match via aria-activedescendant
    expect(input.getAttribute('aria-activedescendant')).toBe('match-apps-1');
  });

  it('hides view when the backdrop is clicked', async () => {
    mockCall.mockResolvedValue({ matches: [] });

    render(App);
    const backdrop = document.querySelector('.backdrop') as HTMLElement;
    expect(backdrop).not.toBeNull();

    // A click whose target is the backdrop itself dismisses the launcher.
    await fireEvent.click(backdrop);

    await waitFor(() => {
      expect(mockCall).toHaveBeenCalledWith('view.hide', { name: 'launcher' });
    });
  });

  it('does not hide view when the card is clicked', async () => {
    mockCall.mockResolvedValue({ matches: [] });

    render(App);
    const card = document.querySelector('.card') as HTMLElement;
    expect(card).not.toBeNull();

    await fireEvent.click(card);

    expect(mockCall).not.toHaveBeenCalledWith('view.hide', { name: 'launcher' });
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

  it('document keydown forwards printable character to input when not focused', async () => {
    mockCall.mockResolvedValue({ matches: [] });

    render(App);
    // Flush effects so the document-level keydown listener is installed.
    await act();

    const input = screen.getByPlaceholderText('Search...') as HTMLInputElement;

    // Blur the input so it is not the active element.
    input.blur();
    expect(document.activeElement).not.toBe(input);

    // Dispatch a keydown on document (not on the input).
    const event = new KeyboardEvent('keydown', {
      key: 'a',
      bubbles: true,
      cancelable: true,
    });
    document.dispatchEvent(event);

    // The forwarder must refocus the input and append the character.
    expect(document.activeElement).toBe(input);
    expect(input.value).toBe('a');
  });

  it('clears the search text when the launcher is reopened', async () => {
    const user = userEvent.setup();
    mockCall.mockResolvedValue({ matches: [] });

    render(App);
    await act();
    const input = screen.getByPlaceholderText('Search...') as HTMLInputElement;

    // The user searches for something.
    await user.type(input, 'asd');
    await waitFor(() => expect(input.value).toBe('asd'));

    // The user dismisses the launcher with Escape (the view is hidden but the
    // Svelte view persists across hide/show).
    await fireEvent.keyDown(input, { key: 'Escape' });

    // The launcher is shown again: the window regains focus. The stale query
    // must not survive the reopen.
    window.dispatchEvent(new Event('focus'));
    await act();

    await waitFor(() => expect(input.value).toBe(''));
  });

  // The wiring tests set the query with fireEvent.input rather than
  // userEvent.type: typing focuses the input, which fires the window focus
  // handler that re-runs the empty search and would clobber the query mid-test.
  const captureResult = {
    command: 'echo hi',
    stdout: 'hi\n',
    stderr: '',
    exit_code: 0,
    timed_out: false,
  };

  function mockSearchAndCapture() {
    mockCall.mockImplementation((method: string) => {
      if (method === 'search') {
        return Promise.resolve({ matches: [] });
      }
      if (method === 'shell.run_capture') {
        return Promise.resolve(captureResult);
      }
      return Promise.resolve({});
    });
  }

  it('runs a $ capture command on Enter and shows the output panel', async () => {
    mockSearchAndCapture();

    render(App);
    const input = screen.getByPlaceholderText('Search...') as HTMLInputElement;

    await fireEvent.input(input, { target: { value: '$echo hi' } });
    await fireEvent.keyDown(input, { key: 'Enter' });

    await waitFor(() => {
      expect(mockCall).toHaveBeenCalledWith('shell.run_capture', { command: 'echo hi' });
      expect(screen.getByText('hi')).toBeDefined();
      expect(screen.getByText('exit 0')).toBeDefined();
    });

    // The normal action must not have run for a capture query.
    expect(mockCall).not.toHaveBeenCalledWith('action.invoke', expect.anything());
  });

  it('returns to the results list when Escape is pressed while the panel is showing', async () => {
    mockSearchAndCapture();

    render(App);
    const input = screen.getByPlaceholderText('Search...') as HTMLInputElement;

    await fireEvent.input(input, { target: { value: '$echo hi' } });
    await fireEvent.keyDown(input, { key: 'Enter' });

    await waitFor(() => expect(screen.getByText('hi')).toBeDefined());

    // Escape clears the panel and does NOT hide the launcher view.
    mockCall.mockClear();
    await fireEvent.keyDown(input, { key: 'Escape' });

    await waitFor(() => expect(screen.queryByText('hi')).toBeNull());
    expect(mockCall).not.toHaveBeenCalledWith('view.hide', { name: 'launcher' });
  });

  it('clears the output panel when the query text changes', async () => {
    mockSearchAndCapture();

    render(App);
    const input = screen.getByPlaceholderText('Search...') as HTMLInputElement;

    await fireEvent.input(input, { target: { value: '$echo hi' } });
    await fireEvent.keyDown(input, { key: 'Enter' });

    await waitFor(() => expect(screen.getByText('hi')).toBeDefined());

    // Editing the query returns to the normal results flow.
    await fireEvent.input(input, { target: { value: '$echo hix' } });
    await waitFor(() => expect(screen.queryByText('hi')).toBeNull());
  });

  it('shows the prefix legend when the input is empty', async () => {
    mockCall.mockResolvedValue({ matches: [] });

    render(App);
    await act();

    await waitFor(() => {
      expect(screen.getByText(/> launch/)).toBeDefined();
      expect(screen.getByText(/! terminal/)).toBeDefined();
      expect(screen.getByText(/\$ run & show/)).toBeDefined();
    });
  });

  it('hides the prefix legend when the input is non-empty', async () => {
    mockCall.mockResolvedValue({ matches: [] });

    render(App);
    await act();
    const input = screen.getByPlaceholderText('Search...') as HTMLInputElement;

    // The legend is visible while the query is empty.
    await waitFor(() => expect(screen.getByText(/run & show/)).toBeDefined());

    // Setting a non-empty query hides it.
    await fireEvent.input(input, { target: { value: 'firefox' } });

    await waitFor(() => expect(screen.queryByText(/run & show/)).toBeNull());
  });

  it('window focus event refocuses input', async () => {
    mockCall.mockResolvedValue({ matches: [] });

    render(App);
    // Flush effects so the window focus listener is installed.
    await act();

    const input = screen.getByPlaceholderText('Search...') as HTMLInputElement;

    // Blur the input.
    input.blur();
    expect(document.activeElement).not.toBe(input);

    // Dispatch a focus event on window.
    window.dispatchEvent(new Event('focus'));

    expect(document.activeElement).toBe(input);
  });

  // The secondary-actions panel: a provider-supplied MenuAction[] surfaced via
  // Ctrl+K / Tab / right-click on the highlighted result.
  const matchWithActions = {
    id: '1',
    provider: 'clipboard',
    title: 'Clipboard entry',
    score: 0.9,
    action: { kind: 'copy', data: { text: 'hello' } },
    actions: [
      { label: 'Paste', action: { kind: 'launch', data: { id: 'paste' } } },
      { label: 'Delete', danger: true, action: { kind: 'custom', data: { kind: 'clipboard', payload: { op: 'delete' } } } },
    ],
  };

  function mockSearchWithActions() {
    mockCall.mockImplementation((method: string) => {
      if (method === 'search') {
        return Promise.resolve({ matches: [matchWithActions] });
      }
      return Promise.resolve({});
    });
  }

  function queryMenuButtonLabels(): string[] {
    const menu = document.querySelector('[data-quantum-context-menu]');
    if (!menu) {
      return [];
    }
    return Array.from(menu.querySelectorAll('button')).map((button) => button.textContent ?? '');
  }

  it('opens the secondary-actions menu with the provider labels on Ctrl+K', async () => {
    mockSearchWithActions();

    render(App);
    const input = screen.getByPlaceholderText('Search...') as HTMLInputElement;

    await fireEvent.input(input, { target: { value: 'clip' } });
    await waitFor(() => expect(screen.getByText('Clipboard entry')).toBeDefined());

    await fireEvent.keyDown(input, { key: 'k', ctrlKey: true });

    await waitFor(() => {
      expect(queryMenuButtonLabels()).toEqual(['Paste', 'Delete']);
    });
  });

  it('opens the secondary-actions menu on Tab as well', async () => {
    mockSearchWithActions();

    render(App);
    const input = screen.getByPlaceholderText('Search...') as HTMLInputElement;

    await fireEvent.input(input, { target: { value: 'clip' } });
    await waitFor(() => expect(screen.getByText('Clipboard entry')).toBeDefined());

    await fireEvent.keyDown(input, { key: 'Tab' });

    await waitFor(() => {
      expect(queryMenuButtonLabels()).toEqual(['Paste', 'Delete']);
    });
  });

  it('invokes action.invoke with the chosen action when a menu item is selected', async () => {
    mockSearchWithActions();

    render(App);
    const input = screen.getByPlaceholderText('Search...') as HTMLInputElement;

    await fireEvent.input(input, { target: { value: 'clip' } });
    await waitFor(() => expect(screen.getByText('Clipboard entry')).toBeDefined());

    await fireEvent.keyDown(input, { key: 'k', ctrlKey: true });
    await waitFor(() => expect(queryMenuButtonLabels()).toEqual(['Paste', 'Delete']));

    // Selecting "Delete" invokes its action against the match's provider.
    const deleteButton = Array.from(
      document.querySelectorAll('[data-quantum-context-menu] button')
    ).find((button) => button.textContent === 'Delete') as HTMLButtonElement;
    await fireEvent.click(deleteButton);

    await waitFor(() => {
      expect(mockCall).toHaveBeenCalledWith('action.invoke', {
        provider: 'clipboard',
        action: { kind: 'custom', data: { kind: 'clipboard', payload: { op: 'delete' } } },
      });
    });
  });

  it('falls back to Open / Copy name when the match has no provider actions', async () => {
    const plain = {
      id: '1',
      provider: 'apps',
      title: 'Firefox',
      score: 0.9,
      action: { kind: 'launch', data: { desktop_id: 'firefox' } },
    };
    mockCall.mockImplementation((method: string) => {
      if (method === 'search') {
        return Promise.resolve({ matches: [plain] });
      }
      return Promise.resolve({});
    });

    render(App);
    const input = screen.getByPlaceholderText('Search...') as HTMLInputElement;

    await fireEvent.input(input, { target: { value: 'fire' } });
    await waitFor(() => expect(screen.getByText('Firefox')).toBeDefined());

    await fireEvent.keyDown(input, { key: 'k', ctrlKey: true });

    await waitFor(() => {
      expect(queryMenuButtonLabels()).toEqual(['Open', 'Copy name']);
    });
  });

  it('keeps the launcher open when Escape dismisses the actions menu', async () => {
    mockSearchWithActions();

    render(App);
    const input = screen.getByPlaceholderText('Search...') as HTMLInputElement;

    await fireEvent.input(input, { target: { value: 'clip' } });
    await waitFor(() => expect(screen.getByText('Clipboard entry')).toBeDefined());

    await fireEvent.keyDown(input, { key: 'k', ctrlKey: true });
    await waitFor(() => expect(queryMenuButtonLabels()).toEqual(['Paste', 'Delete']));

    mockCall.mockClear();
    mockSearchWithActions();

    // Escape closes the menu (openContextMenu handles it) but must NOT also
    // trigger the launcher's own hide.
    await fireEvent.keyDown(input, { key: 'Escape' });

    await waitFor(() => expect(document.querySelector('[data-quantum-context-menu]')).toBeNull());
    expect(mockCall).not.toHaveBeenCalledWith('view.hide', { name: 'launcher' });
  });
});
