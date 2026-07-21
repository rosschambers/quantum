<script lang="ts">
  // Consuming real @quantum/client APIs for search, action.invoke, and view.hide.
  // In browser, the client auto-detects WebKit bridge. In tests, it's mocked.
  import { createClient, openContextMenu } from '@quantum/client';
  import type { ShellCaptureResult, MenuItem } from '@quantum/client';
  import { untrack } from 'svelte';
  import SearchInput from './lib/SearchInput.svelte';
  import Results from './lib/Results.svelte';
  import CommandOutput from './lib/CommandOutput.svelte';
  import { parseCommandQuery } from './lib/commandQuery';
  import { providersForQuery } from './lib/prefixMode';
  import type { Match, MenuAction } from './lib/types';

  interface ThemeReloadedPayload {
    css: string;
  }

  const client = createClient();

  let searchText = $state('');
  let matches: Match[] = $state([]);
  let highlightedIndex = $state(0);
  let isLoading = $state(false);
  let lastSearchTimeout: number | undefined;

  // The `$` command-capture panel. Non-null while a captured command is either
  // running or showing its result, in which case it replaces the results list.
  let capture = $state<{ running: boolean; result: ShellCaptureResult | null } | null>(null);

  // True while the secondary-actions menu is open. openContextMenu installs its
  // own capture-phase Escape handler that closes the menu without stopping
  // propagation, so the launcher's Escape handler would otherwise also fire and
  // hide the whole launcher. This flag lets that handler no-op while the menu is
  // dismissing. It is cleared on a microtask (not synchronously) inside the
  // menu's onClose so the launcher keydown that follows in the same event still
  // sees the menu as having been open.
  let actionsMenuOpen = false;

  // The SearchInput's onInput handler: a genuine user edit returns to the
  // normal results flow, so any showing command-output panel is cleared before
  // the search runs. This is deliberately separate from handleSearch, which is
  // also called programmatically (mount and reopen) where the panel must not be
  // clobbered by an incidental re-render.
  function handleUserInput(text: string) {
    capture = null;
    handleSearch(text);
  }

  function handleSearch(text: string) {
    searchText = text;

    if (lastSearchTimeout !== undefined) {
      clearTimeout(lastSearchTimeout);
    }

    // Prefix dispatch decides which providers the query is pinned to: an empty
    // query fetches the default (usage-ranked) apps, the punctuation prefixes
    // (`=` calc, `:` emoji, `;` clipboard, `>`/`!` shell) route to a single
    // provider so that mode's result is the sole, top answer, and any other
    // query fans out to all providers. The `$` capture path is separate and
    // handled on Enter, not here.
    const providers = providersForQuery(text);

    isLoading = true;
    lastSearchTimeout = window.setTimeout(async () => {
      try {
        const response = await client.call('search', {
          text,
          providers,
        });

        matches = response?.matches || [];
        highlightedIndex = 0;
      } catch (error) {
        console.error('Search failed:', error);
        matches = [];
      } finally {
        isLoading = false;
      }
    }, 50);
  }

  function handleKeyDown(event: KeyboardEvent) {
    if ((event.key === 'k' && (event.ctrlKey || event.metaKey)) || event.key === 'Tab') {
      // Ctrl+K / Meta+K / Tab open the secondary-actions menu for the
      // highlighted result. Tab's default (focus traversal) is suppressed.
      event.preventDefault();
      openActionsMenuForHighlighted();
      return;
    }
    if (event.key === 'ArrowDown') {
      event.preventDefault();
      highlightedIndex = Math.min(highlightedIndex + 1, matches.length - 1);
    } else if (event.key === 'ArrowUp') {
      event.preventDefault();
      highlightedIndex = Math.max(highlightedIndex - 1, 0);
    } else if (event.key === 'Enter') {
      event.preventDefault();
      const query = parseCommandQuery(searchText);
      if (query.mode === 'capture') {
        // A `$` command runs and shows its output inline instead of invoking
        // the normally-selected action.
        runCapture(query.command);
      } else if (matches.length > 0 && highlightedIndex >= 0 && highlightedIndex < matches.length) {
        const selected = matches[highlightedIndex];
        invokeAction(selected);
      }
    } else if (event.key === 'Escape') {
      if (actionsMenuOpen) {
        // The secondary-actions menu is open: openContextMenu's own Escape
        // handler already closed it. Do nothing here so the launcher stays
        // open (and do not preventDefault, so the menu's handler is unaffected).
        return;
      }
      event.preventDefault();
      if (capture !== null) {
        // A showing command-output panel is dismissed first; the launcher
        // stays open so the user can keep typing.
        capture = null;
      } else {
        client.call('view.hide', { name: 'launcher' }).catch(console.error);
      }
    }
  }

  async function runCapture(command: string) {
    capture = { running: true, result: null };
    try {
      const result = (await client.call('shell.run_capture', { command })) as ShellCaptureResult;
      capture = { running: false, result };
    } catch (error) {
      // A failed IPC call still resolves to a non-crashing panel: surface the
      // error in the standard-error block with a failing exit code.
      capture = {
        running: false,
        result: {
          command,
          stdout: '',
          stderr: String(error),
          exit_code: -1,
          timed_out: false,
        },
      };
    }
  }

  function onBackdropClick(event: MouseEvent) {
    // Only dismiss when the click lands on the backdrop itself, not on the
    // card or its children (which bubble up to this handler).
    if (event.target === event.currentTarget) {
      client.call('view.hide', { name: 'launcher' }).catch(console.error);
    }
  }

  async function invokeAction(match: Match) {
    try {
      await client.call('action.invoke', {
        provider: match.provider,
        action: match.action,
      });
      searchText = '';
      matches = [];
      await client.call('view.hide', { name: 'launcher' });
    } catch (error) {
      console.error('Action invocation failed:', error);
    }
  }

  // Invoke a provider-supplied secondary action against its match. A launch or
  // focus action is terminal (it opens or switches to something), so the
  // launcher hides afterwards as it does for a primary Enter. A copy or any
  // provider `custom` operation (a clipboard delete, a "copy path") is
  // non-terminal: the user is likely to keep working in the launcher, so it
  // stays open. This mirrors the primary-action behaviour where launching
  // dismisses but capturing does not.
  async function invokeMenuAction(match: Match, menuAction: MenuAction) {
    try {
      await client.call('action.invoke', {
        provider: match.provider,
        action: menuAction.action,
      });
      const kind = menuAction.action.kind;
      const isTerminal = kind === 'launch' || kind === 'focus';
      if (isTerminal) {
        searchText = '';
        matches = [];
        await client.call('view.hide', { name: 'launcher' });
      }
    } catch (error) {
      console.error('Secondary action invocation failed:', error);
    }
  }

  // Build the menu items for a match: its provider-supplied secondary actions
  // when present, otherwise the default Open / Copy name pair so every result
  // still has a menu (today's behaviour).
  function buildActionsMenu(match: Match): MenuItem[] {
    if (match.actions && match.actions.length > 0) {
      return match.actions.map((menuAction) => ({
        label: menuAction.label,
        icon: menuAction.icon,
        danger: menuAction.danger,
        onSelect: () => invokeMenuAction(match, menuAction),
      }));
    }
    return [
      { label: 'Open', onSelect: () => invokeAction(match) },
      {
        label: 'Copy name',
        onSelect: () => {
          navigator.clipboard?.writeText(match.title).catch(() => {});
        },
      },
    ];
  }

  // Open the actions menu, tracking open state so Escape closes only the menu
  // (not the launcher). `anchorRect` drops the menu below the given row for the
  // keyboard path; a right-click passes no anchor so it uses the cursor.
  function openActionsMenu(
    event: MouseEvent,
    match: Match,
    anchorRect?: { x: number; y: number; width: number; height: number }
  ) {
    actionsMenuOpen = true;
    openContextMenu(event, buildActionsMenu(match), {
      anchorRect,
      onClose: () => {
        // Cleared asynchronously: the Escape keydown that closes the menu also
        // bubbles to the launcher's handler in the same tick, which must still
        // read `actionsMenuOpen` as true to avoid hiding the launcher.
        queueMicrotask(() => {
          actionsMenuOpen = false;
        });
      },
    });
  }

  // Right-click a result opens its actions menu at the cursor.
  function resultMenu(event: MouseEvent, match: Match) {
    openActionsMenu(event, match);
  }

  // Open the actions menu for the currently highlighted result via the
  // keyboard (Ctrl+K / Meta+K / Tab). openContextMenu needs a MouseEvent, so a
  // synthetic one is built; the menu is anchored to the highlighted row's
  // bounding box so it drops down from that row rather than an arbitrary point.
  function openActionsMenuForHighlighted() {
    if (matches.length === 0 || highlightedIndex < 0 || highlightedIndex >= matches.length) {
      return;
    }
    const match = matches[highlightedIndex];
    const row = document.getElementById(`match-${match.provider}-${match.id}`);
    const rect = row?.getBoundingClientRect();
    const anchorRect = rect
      ? { x: rect.x, y: rect.y, width: rect.width, height: rect.height }
      : undefined;
    // A non-dispatched MouseEvent has a null target, so openContextMenu falls
    // back to the ambient document; its clientX/Y are ignored when anchorRect
    // is supplied, and its preventDefault/stopPropagation are harmless no-ops.
    const syntheticEvent = new MouseEvent('contextmenu', {
      clientX: anchorRect ? anchorRect.x : 0,
      clientY: anchorRect ? anchorRect.y + anchorRect.height : 0,
    });
    openActionsMenu(syntheticEvent, match, anchorRect);
  }

  $effect(() => {
    // Clamp highlighted index if matches length changes
    if (matches.length > 0) {
      highlightedIndex = Math.min(highlightedIndex, matches.length - 1);
    } else {
      highlightedIndex = 0;
    }
  });

  let activeDescendant = $derived(
    matches.length > 0 && matches[highlightedIndex]
      ? `match-${matches[highlightedIndex].provider}-${matches[highlightedIndex].id}`
      : undefined
  );

  $effect(() => {
    // Setup work runs in $effect, not onMount: testing-library's Svelte 5
    // adapter does not fire onMount reliably under runes mode, and the
    // show-handling listeners below must run for the reopen-clears-search
    // behaviour to hold.
    //
    // Subscribe to theme reload notifications and update CSS tokens in place
    const unsubscribe = client.subscribe('theme.reloaded', (payload: unknown) => {
      const p = payload as ThemeReloadedPayload;
      const style = document.getElementById('quantum-tokens');
      if (style && typeof p?.css === 'string') {
        style.textContent = p.css;
      }
    });

    // Reset to a fresh, empty query every time the launcher is shown again. The
    // view persists across hide/show, so stale search text (and its results)
    // would otherwise survive a dismiss/reopen — type "asd", press Escape, and
    // it would reappear still showing "asd". Clearing to an empty query on show
    // also self-heals the list: the one-shot mount query below is not enough on
    // its own because if it ever returns empty (it raced the IPC bridge, or a
    // `nixos-rebuild switch` churned the daemon) the list would stay empty until
    // the daemon recreated the view. Re-running the empty search when the window
    // regains focus or becomes visible clears any stale query and refreshes the
    // default (usage-ranked) apps each open.
    const requeryOnShow = () => {
      // Reopening the launcher starts fresh: drop any command-output panel.
      capture = null;
      handleSearch('');
    };
    const onVisibility = () => {
      if (document.visibilityState === 'visible') {
        requeryOnShow();
      }
    };
    window.addEventListener('focus', requeryOnShow);
    document.addEventListener('visibilitychange', onVisibility);

    return () => {
      unsubscribe?.();
      window.removeEventListener('focus', requeryOnShow);
      document.removeEventListener('visibilitychange', onVisibility);
    };
  });

  $effect(() => {
    // On open, the launcher mounts fresh with an empty query: fetch the
    // default (usage-ranked) apps so something useful shows before any typing.
    // `untrack` keeps this a one-shot setup with no tracked dependencies.
    untrack(() => handleSearch(''));
  });
</script>

<!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
<div class="backdrop" onclick={onBackdropClick}>
  <div class="card" role="dialog" aria-label="Application launcher">
    <div class="search-container">
      <SearchInput
        value={searchText}
        onInput={handleUserInput}
        onKeyDown={handleKeyDown}
        expanded={matches.length > 0}
        activeDescendant={activeDescendant}
      />
    </div>

    {#if !searchText.trim()}
      <!-- An empty input shows a one-line legend of the command prefixes so
           the `>`, `!`, `$`, `=`, `:`, and `;` modes are discoverable; it hides
           the moment the user types anything. -->
      <div class="prefix-legend">&gt; launch &nbsp;·&nbsp; ! terminal &nbsp;·&nbsp; $ run &amp; show &nbsp;·&nbsp; = calc &nbsp;·&nbsp; : emoji &nbsp;·&nbsp; ; clipboard</div>
    {/if}

    <div class="results-container">
      {#if capture !== null}
        <CommandOutput running={capture.running} result={capture.result} />
      {:else if matches.length > 0}
        <Results
          items={matches}
          highlighted={highlightedIndex}
          onSelect={invokeAction}
          onContext={resultMenu}
        />
      {:else if isLoading}
        <div class="loading">Searching...</div>
      {:else if searchText.trim()}
        <div class="empty-state">No results found</div>
      {:else}
        <div class="empty-state">No applications found</div>
      {/if}
    </div>
  </div>
</div>

<style>
  :global(body) {
    margin: 0;
    padding: 0;
  }
</style>
