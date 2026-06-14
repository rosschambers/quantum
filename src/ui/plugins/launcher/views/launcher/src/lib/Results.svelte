<script lang="ts">
  import type { Match, IconRef } from './types';

  interface Props {
    items: Match[];
    highlighted: number;
    onSelect: (item: Match) => void;
  }

  let { items = [], highlighted = 0, onSelect }: Props = $props();

  function handleClick(item: Match) {
    onSelect(item);
  }

  /** Resolve an IconRef to a URL string, or undefined. */
  function resolveIcon(icon: string | IconRef | undefined): string | undefined {
    if (!icon) {
      return undefined;
    }
    if (typeof icon === 'string') {
      return icon;
    }
    if (icon.kind === 'name') {
      // Use the freedesktop icon theme URI scheme which is
      // supported by most browsers on Linux.
      // Try multiple standard paths that browsers support.
      return `gnome-icon-theme/${icon.data}`;
    }
    if (icon.kind === 'path') {
      return `file://${icon.data}`;
    }
    if (icon.kind === 'data_uri') {
      return icon.data;
    }
    return undefined;
  }
</script>

<div class="results-list" id="quantum-results" role="listbox">
  {#each items as item, index (item.provider + ':' + item.id)}
    <!-- Keyboard activation is on the parent SearchInput via Enter; the
         click handler here is a mouse affordance only. -->
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div
      class="match-item"
      class:highlighted={index === highlighted}
      id={`match-${item.provider}-${item.id}`}
      role="option"
      tabindex="-1"
      aria-selected={index === highlighted ? 'true' : 'false'}
      onclick={() => handleClick(item)}
    >
      {#if resolveIcon(item.icon)}
        <img class="icon" src={resolveIcon(item.icon)} alt="" loading="lazy" onerror={() => (item.icon = undefined)} />
      {:else}
        <div class="icon"></div>
      {/if}
      <div class="title">{item.title}</div>
      {#if item.subtitle}
        <div class="subtitle">{item.subtitle}</div>
      {/if}
    </div>
  {/each}
</div>

<style>
  .results-list {
    display: flex;
    flex-direction: column;
    gap: 0;
  }
</style>
