<script lang="ts">
  import { onMount } from 'svelte';

  interface Props {
    value: string;
    onInput: (text: string) => void;
    onKeyDown: (event: KeyboardEvent) => void;
    activeDescendant?: string;
    expanded?: boolean;
  }

  let {
    value = $bindable(),
    onInput,
    onKeyDown,
    activeDescendant,
    expanded = false,
  }: Props = $props();

  let inputElement: HTMLInputElement;

  onMount(() => {
    if (inputElement) {
      inputElement.focus();
    }
  });

  function handleInput(event: Event) {
    const target = event.target as HTMLInputElement;
    value = target.value;
    onInput(value);
  }

  function handleKeyDown(event: KeyboardEvent) {
    onKeyDown(event);
  }
</script>

<input
  bind:this={inputElement}
  type="text"
  placeholder="Search..."
  value={value}
  role="combobox"
  aria-expanded={expanded}
  aria-controls="quantum-results"
  aria-autocomplete="list"
  aria-activedescendant={activeDescendant}
  oninput={handleInput}
  onkeydown={handleKeyDown}
/>
