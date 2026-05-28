<script lang="ts">
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

<div class="results-list">
  {#each items as item, index (item.provider + ':' + item.id + ':' + index)}
    <div
      class="match-item"
      class:highlighted={index === highlighted}
      on:click={() => handleClick(item)}
      role="option"
      aria-selected={index === highlighted}
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
