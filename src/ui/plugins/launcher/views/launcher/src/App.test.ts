import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
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
    // Mirror the real onShown: register a window listener for `quantum:shown`
    // so a test can fire the host's re-show signal.
    onShown: (callback: () => void) => {
      const handler = (): void => callback();
      window.addEventListener('quantum:shown', handler);
      return () => window.removeEventListener('quantum:shown', handler);
    },
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

  it('scrolls the newly highlighted row into view on ArrowDown', async () => {
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
    // happy-dom has no scrollIntoView; record which element each call targets.
    const scrolled: { element: Element; options: unknown }[] = [];
    const prototype = Element.prototype as { scrollIntoView?: unknown };
    const original = prototype.scrollIntoView;
    prototype.scrollIntoView = vi.fn(function (this: Element, options: unknown) {
      scrolled.push({ element: this, options });
    });
    try {
      render(App);
      const input = screen.getByPlaceholderText('Search...');
      await fireEvent.input(input, { target: { value: 'x' } });
      await waitFor(() => expect(screen.getByText('Chrome')).toBeDefined());

      scrolled.length = 0;
      await fireEvent.keyDown(input, { key: 'ArrowDown' });

      await waitFor(() => expect(scrolled.length).toBeGreaterThan(0));
      const last = scrolled[scrolled.length - 1];
      expect(last.element).toBe(screen.getByText('Chrome').closest('.match-item'));
      expect(last.options).toEqual({ block: 'nearest' });
    } finally {
      if (original === undefined) {
        delete prototype.scrollIntoView;
      } else {
        prototype.scrollIntoView = original;
      }
    }
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

// Clipboard mode (`;`) widens the card and previews the highlighted entry
// beside the list. Queries are set with fireEvent.input rather than
// userEvent.type, for the focus-handler reason given above.
describe('App.svelte clipboard preview', () => {
  beforeEach(() => {
    mockCall = vi.fn();
  });

  const clipboardMatches = [
    {
      id: 'a',
      provider: 'clipboard',
      title: 'first entry line one line two',
      subtitle: '1m ago',
      score: 1,
      action: { kind: 'copy', data: { text: 'first entry\nline one\nline two' } },
    },
    {
      id: 'b',
      provider: 'clipboard',
      title: 'second entry',
      subtitle: '9m ago',
      score: 1,
      action: { kind: 'copy', data: { text: 'second entry' } },
    },
  ];

  function mockClipboardSearch() {
    mockCall.mockImplementation((method: string, params: { providers?: string[] }) => {
      if (method === 'search' && params?.providers?.[0] === 'clipboard') {
        return Promise.resolve({ matches: clipboardMatches });
      }
      if (method === 'shell.run_capture') {
        return Promise.resolve({
          command: 'echo hi',
          stdout: 'hi\n',
          stderr: '',
          exit_code: 0,
          timed_out: false,
        });
      }
      return Promise.resolve({ matches: [] });
    });
  }

  it('shows the highlighted entry in a preview pane in clipboard mode', async () => {
    mockClipboardSearch();
    const { container } = render(App);
    await act();
    const input = screen.getByPlaceholderText('Search...');
    await fireEvent.input(input, { target: { value: ';' } });
    await waitFor(() =>
      expect(container.querySelector('.clipboard-preview-text')?.textContent).toBe(
        'first entry\nline one\nline two'
      )
    );
    expect(container.querySelector('.card')?.classList.contains('clipboard-mode')).toBe(true);
    expect(container.querySelector('.clipboard-preview-details')?.textContent).toBe(
      '1m ago · 29 bytes · 3 lines'
    );
  });

  it('moves the preview with the highlight', async () => {
    mockClipboardSearch();
    const { container } = render(App);
    await act();
    const input = screen.getByPlaceholderText('Search...');
    await fireEvent.input(input, { target: { value: ';' } });
    await waitFor(() => expect(container.querySelector('.clipboard-preview-text')).not.toBeNull());
    await fireEvent.keyDown(input, { key: 'ArrowDown' });
    await waitFor(() =>
      expect(container.querySelector('.clipboard-preview-text')?.textContent).toBe('second entry')
    );
  });

  it('has no preview pane outside clipboard mode', async () => {
    mockClipboardSearch();
    const { container } = render(App);
    await act();
    const input = screen.getByPlaceholderText('Search...');
    await fireEvent.input(input, { target: { value: 'fire' } });
    await act();
    expect(container.querySelector('.clipboard-preview')).toBeNull();
    expect(container.querySelector('.card')?.classList.contains('clipboard-mode')).toBe(false);
  });

  it('has no preview pane while the command output panel shows', async () => {
    mockClipboardSearch();
    const { container } = render(App);
    await act();
    const input = screen.getByPlaceholderText('Search...');
    await fireEvent.input(input, { target: { value: ';' } });
    await waitFor(() => expect(container.querySelector('.clipboard-preview')).not.toBeNull());
    await fireEvent.input(input, { target: { value: '$echo hi' } });
    await fireEvent.keyDown(input, { key: 'Enter' });
    await waitFor(() => expect(screen.getByText('exit 0')).toBeDefined());
    expect(container.querySelector('.clipboard-preview')).toBeNull();
    expect(container.querySelector('.card')?.classList.contains('clipboard-mode')).toBe(false);
  });
});

// Stand-in for the host injecting `window.__quantum_args` before a show.
function setOpenArgs(value: unknown): void {
  (window as { __quantum_args?: unknown }).__quantum_args = value;
}

describe('App.svelte open arguments', () => {
  beforeEach(() => {
    mockCall = vi.fn().mockResolvedValue({ matches: [] });
  });
  afterEach(() => {
    delete (window as { __quantum_args?: unknown }).__quantum_args;
  });

  it('opens in clipboard mode when shown with a ; query argument', async () => {
    setOpenArgs({ query: ';' });
    render(App);
    await act();
    const input = screen.getByPlaceholderText('Search...') as HTMLInputElement;
    await waitFor(() => expect(input.value).toBe(';'));
    await waitFor(() =>
      expect(mockCall).toHaveBeenCalledWith('search', { text: ';', providers: ['clipboard'] })
    );
  });

  it('returns to an empty query when reopened without arguments', async () => {
    setOpenArgs({ query: ';' });
    render(App);
    await act();
    const input = screen.getByPlaceholderText('Search...') as HTMLInputElement;
    await waitFor(() => expect(input.value).toBe(';'));
    setOpenArgs(null);
    window.dispatchEvent(new Event('focus'));
    await act();
    await waitFor(() => expect(input.value).toBe(''));
  });

  it('switches an app search to clipboard mode when reopened with a ; argument', async () => {
    render(App);
    await act();
    const input = screen.getByPlaceholderText('Search...') as HTMLInputElement;
    await fireEvent.input(input, { target: { value: 'fire' } });
    setOpenArgs({ query: ';' });
    window.dispatchEvent(new Event('focus'));
    await act();
    await waitFor(() => expect(input.value).toBe(';'));
    await waitFor(() =>
      expect(mockCall).toHaveBeenCalledWith('search', { text: ';', providers: ['clipboard'] })
    );
  });

  it('switches to clipboard mode when shown again while already focused', async () => {
    // Super+V while the launcher is visible and focused: the window neither
    // regains focus nor changes visibility, so only the host's
    // `quantum:shown` event announces the new open arguments.
    render(App);
    await act();
    const input = screen.getByPlaceholderText('Search...') as HTMLInputElement;
    await fireEvent.input(input, { target: { value: 'fire' } });
    setOpenArgs({ query: ';' });
    window.dispatchEvent(new CustomEvent('quantum:shown'));
    await act();
    await waitFor(() => expect(input.value).toBe(';'));
    await waitFor(() =>
      expect(mockCall).toHaveBeenCalledWith('search', { text: ';', providers: ['clipboard'] })
    );
  });
});
