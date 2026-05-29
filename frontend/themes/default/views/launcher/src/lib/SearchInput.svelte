<script lang="ts">
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

  function focusInput() {
    if (inputElement && document.activeElement !== inputElement) {
      inputElement.focus();
    }
  }

  function forwardKeydownToInput(event: KeyboardEvent) {
    // If the input already has focus, let its own onkeydown handler fire.
    if (document.activeElement === inputElement) {
      return;
    }

    // Refocus so subsequent keystrokes land in the input directly.
    inputElement?.focus();

    // For printable single-character keys (no modifiers other than Shift),
    // synthesize the keystroke by appending it to the value and firing an
    // input event. Non-printable keys (Arrow*, Enter, Escape, Backspace,
    // Tab, etc.) have key.length > 1 and are forwarded to the existing
    // onkeydown handler so navigation works regardless of focus location.
    if (
      event.key.length === 1 &&
      !event.ctrlKey &&
      !event.metaKey &&
      !event.altKey
    ) {
      const newValue = (inputElement.value || '') + event.key;
      inputElement.value = newValue;
      inputElement.dispatchEvent(new Event('input', { bubbles: true }));
      event.preventDefault();
    } else {
      onKeyDown(event);
    }
  }

  function handleWindowFocus() {
    focusInput();
  }

  // Install document/window listeners and the initial focus once the input
  // element is bound. Using $effect rather than onMount because the latter
  // does not run reliably under @testing-library/svelte/svelte5 + happy-dom.
  $effect(() => {
    if (!inputElement) {
      return;
    }

    focusInput();
    document.addEventListener('keydown', forwardKeydownToInput);
    window.addEventListener('focus', handleWindowFocus);

    return () => {
      document.removeEventListener('keydown', forwardKeydownToInput);
      window.removeEventListener('focus', handleWindowFocus);
    };
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
