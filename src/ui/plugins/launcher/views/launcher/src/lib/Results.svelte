<script lang="ts">
  import type { Match } from './types';
  import { resolveIcon, isThumbnailIcon } from './icon';

  interface Props {
    items: Match[];
    highlighted: number;
    onSelect: (item: Match) => void;
    onContext?: (event: MouseEvent, item: Match) => void;
  }

  let { items = [], highlighted = 0, onSelect, onContext }: Props = $props();

  function handleClick(item: Match) {
    onSelect(item);
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
      oncontextmenu={onContext ? (event) => onContext(event, item) : undefined}
    >
      {#if resolveIcon(item.icon)}
        <!-- A data_uri icon (a clipboard image preview or mime thumbnail) gets
             the `thumbnail` class so it cover-crops to a square tile; a path or
             string app glyph keeps the contained rendering. -->
        <img
          class="icon"
          class:thumbnail={isThumbnailIcon(item.icon)}
          src={resolveIcon(item.icon)}
          alt=""
          loading="lazy"
          onerror={() => (item.icon = undefined)}
        />
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
