<script lang="ts">
  // Consuming real @quantum/client APIs for search, action.invoke, and view.hide.
  // In browser, the client auto-detects WebKit bridge. In tests, it's mocked.
  import { createClient, openContextMenu } from '@quantum/client';
  import { onMount, untrack } from 'svelte';
  import SearchInput from './lib/SearchInput.svelte';
  import Results from './lib/Results.svelte';
  import type { Match } from './lib/types';

  interface ThemeReloadedPayload {
    css: string;
  }

  const client = createClient();

  let searchText = $state('');
  let matches: Match[] = $state([]);
  let highlightedIndex = $state(0);
  let isLoading = $state(false);
  let lastSearchTimeout: number | undefined;

  function handleSearch(text: string) {
    searchText = text;

    if (lastSearchTimeout !== undefined) {
      clearTimeout(lastSearchTimeout);
    }

    // An empty query fetches the default (usage-ranked) apps. Pin it to the
    // desktop-apps provider so other providers (shell-command, window
    // switcher) don't fire on empty input. A non-empty query fans out to all
    // providers.
    const trimmed = text.trim();
    const providers = trimmed ? [] : ['desktop-apps'];

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
    if (event.key === 'ArrowDown') {
      event.preventDefault();
      highlightedIndex = Math.min(highlightedIndex + 1, matches.length - 1);
    } else if (event.key === 'ArrowUp') {
      event.preventDefault();
      highlightedIndex = Math.max(highlightedIndex - 1, 0);
    } else if (event.key === 'Enter') {
      event.preventDefault();
      if (matches.length > 0 && highlightedIndex >= 0 && highlightedIndex < matches.length) {
        const selected = matches[highlightedIndex];
        invokeAction(selected);
      }
    } else if (event.key === 'Escape') {
      event.preventDefault();
      client.call('view.hide', { name: 'launcher' }).catch(console.error);
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

  // Right-click a result: open it, or copy its name without launching.
  function resultMenu(event: MouseEvent, match: Match) {
    openContextMenu(event, [
      { label: 'Open', onSelect: () => invokeAction(match) },
      {
        label: 'Copy name',
        onSelect: () => {
          navigator.clipboard?.writeText(match.title).catch(() => {});
        },
      },
    ]);
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

  onMount(() => {
    // Subscribe to theme reload notifications and update CSS tokens in place
    const unsubscribe = client.subscribe('theme.reloaded', (payload: unknown) => {
      const p = payload as ThemeReloadedPayload;
      const style = document.getElementById('quantum-tokens');
      if (style && typeof p?.css === 'string') {
        style.textContent = p.css;
      }
    });

    // Re-query every time the launcher is shown again. The view persists across
    // hide/show, so the one-shot mount query below is not enough on its own: if
    // it ever returns empty (it raced the IPC bridge, or a `nixos-rebuild
    // switch` churned the daemon), the list would stay empty until the daemon
    // recreated the view. Re-running the search when the window regains focus or
    // becomes visible makes it self-heal and also refreshes the list each open.
    const requeryOnShow = () => handleSearch(searchText);
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
        onInput={handleSearch}
        onKeyDown={handleKeyDown}
        expanded={matches.length > 0}
        activeDescendant={activeDescendant}
      />
    </div>

    <div class="results-container">
      {#if matches.length > 0}
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
