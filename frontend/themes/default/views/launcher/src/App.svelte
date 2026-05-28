<script lang="ts">
  // Consuming real @quantum/client APIs for search, action.invoke, and view.hide.
  // In browser, the client auto-detects WebKit bridge. In tests, it's mocked.
  import { createClient } from '@quantum/client';
  import SearchInput from './lib/SearchInput.svelte';
  import Results from './lib/Results.svelte';

  type Match = {
    id: string;
    provider: string;
    title: string;
    subtitle?: string;
    icon?: string;
    score: number;
    action: {
      kind: string;
      data: unknown;
    };
  };

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

    if (!text.trim()) {
      matches = [];
      highlightedIndex = 0;
      return;
    }

    isLoading = true;
    lastSearchTimeout = window.setTimeout(async () => {
      try {
        const response = await client.call('search', {
          text,
          providers: [],
        });

        matches = response.matches || [];
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
      client.call('view.hide', { view: 'launcher' }).catch(console.error);
    }
  }

  async function invokeAction(match: Match) {
    try {
      await client.call('action.invoke', {
        provider: match.provider,
        action: match.action,
      });
      await client.call('view.hide', { view: 'launcher' });
    } catch (error) {
      console.error('Action invocation failed:', error);
    }
  }

  $effect(() => {
    // Clamp highlighted index if matches length changes
    if (matches.length > 0) {
      highlightedIndex = Math.min(highlightedIndex, matches.length - 1);
    } else {
      highlightedIndex = 0;
    }
  });
</script>

<div class="search-container">
  <SearchInput
    value={searchText}
    onInput={handleSearch}
    onKeyDown={handleKeyDown}
  />
</div>

<div class="results-container">
  {#if isLoading}
    <div class="loading">Searching...</div>
  {:else if matches.length === 0 && searchText.trim()}
    <div class="empty-state">No results found</div>
  {:else if matches.length > 0}
    <Results
      items={matches}
      highlighted={highlightedIndex}
      onSelect={invokeAction}
    />
  {:else}
    <div class="empty-state">Start typing to search...</div>
  {/if}
</div>

<style>
  :global(body) {
    margin: 0;
    padding: 0;
  }
</style>
