<script lang="ts">
    interface Props {
        percent: number;
        onCommit: (percent: number) => void;
    }
    let { percent, onCommit }: Props = $props();

    /**
     * Local echo: while dragging, the slider shows the drag value and
     * ignores incoming state updates so an event storm during playback
     * cannot make the thumb stutter. After release the provider state
     * (reconciled via audio.event) is the single source of truth again.
     */
    let dragging = $state(false);
    let localPercent = $state(0);
    let debounceTimer: ReturnType<typeof setTimeout> | null = null;

    const DEBOUNCE_MILLISECONDS = 150;

    function onInput(event: Event): void {
        const target = event.currentTarget as HTMLInputElement;
        dragging = true;
        localPercent = Number(target.value);
        if (debounceTimer !== null) clearTimeout(debounceTimer);
        debounceTimer = setTimeout(() => {
            debounceTimer = null;
            onCommit(localPercent);
        }, DEBOUNCE_MILLISECONDS);
    }

    function onRelease(): void {
        if (!dragging) return;
        if (debounceTimer !== null) {
            clearTimeout(debounceTimer);
            debounceTimer = null;
        }
        onCommit(localPercent);
        dragging = false;
    }
</script>

<input
    class="slider"
    type="range"
    min="0"
    max="150"
    value={dragging ? localPercent : percent}
    oninput={onInput}
    onchange={onRelease}
/>

<style>
    .slider {
        flex: 1;
        min-width: 90px;
        accent-color: var(--color-accent, #89b4fa);
    }
</style>
