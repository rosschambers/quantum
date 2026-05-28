<script lang="ts">
  import type { Match } from './types';

  interface Props {
    items: Match[];
    highlighted: number;
    onSelect: (item: Match) => void;
  }

  let { items = [], highlighted = 0, onSelect }: Props = $props();

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
    >
      <div class="icon"></div>
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
