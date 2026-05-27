<script lang="ts">
  import { onMount } from 'svelte';

  interface Props {
    value: string;
    onInput: (text: string) => void;
    onKeyDown: (event: KeyboardEvent) => void;
  }

  let { value = $bindable(), onInput, onKeyDown }: Props = $props();

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
  on:input={handleInput}
  on:keydown={handleKeyDown}
/>
