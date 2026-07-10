<script lang="ts">
    /**
     * The path bar. In its default state it renders the current path as a row
     * of clickable segments (root "/" plus each directory name), separated by
     * chevrons; clicking a segment navigates to the absolute path up to and
     * including it. A keyboard-accessible filler button fills the empty
     * trailing space, so clicking that empty area flips the bar into an
     * editable text input prefilled with the path.
     *
     * The `editing` prop is bindable so the parent App can flip it on Ctrl+L
     * (Task 24); the filler button flips the same state internally. While
     * editing, Enter navigates to the typed value, while Escape and blur both
     * exit without navigating. Validating the typed path is the App's concern —
     * this component only emits it through `onNavigate`.
     */
    import Icon from './Icon.svelte';
    import { breadcrumbSegments } from './path';

    interface Props {
        /** The current absolute directory path. */
        path: string;
        /** Called with an absolute path when the user picks a segment or submits the input. */
        onNavigate: (path: string) => void;
        /** Whether the editable input is shown. Bindable so the App can trigger editing. */
        editing?: boolean;
    }

    let { path, onNavigate, editing = $bindable(false) }: Props = $props();

    const segments = $derived(breadcrumbSegments(path));

    let inputValue = $state('');
    let inputElement = $state<HTMLInputElement | null>(null);

    // When editing turns on, seed the field with the current path and focus +
    // select it so the first keystroke replaces the whole path. The effect
    // does not depend on `inputValue`, so it never clobbers what the user types.
    $effect(() => {
        if (!editing) {
            return;
        }
        inputValue = path;
        const element = inputElement;
        if (element !== null) {
            element.focus();
            element.select();
        }
    });

    function handleSegmentClick(target: string): void {
        onNavigate(target);
    }

    function startEditing(): void {
        editing = true;
    }

    function handleKeydown(event: KeyboardEvent): void {
        // Keep path-bar typing out of the App's global key handling (type-ahead,
        // Alt-arrows) while the input is focused.
        event.stopPropagation();
        if (event.key === 'Enter') {
            editing = false;
            onNavigate(inputValue);
        } else if (event.key === 'Escape') {
            editing = false;
        }
    }

    function handleBlur(): void {
        editing = false;
    }
</script>

<div class="crumbs" title="Click a segment to jump; Ctrl+L to edit">
    {#if editing}
        <input
            class="path-input"
            bind:this={inputElement}
            bind:value={inputValue}
            onkeydown={handleKeydown}
            onblur={handleBlur}
            spellcheck="false"
            autocomplete="off"
        />
    {:else}
        {#each segments as segment, index (segment.target)}
            <button
                type="button"
                class="seg"
                class:last={index === segments.length - 1}
                onclick={() => handleSegmentClick(segment.target)}
            >
                {segment.label}
            </button>
            {#if index < segments.length - 1}
                <span class="sep"><Icon name="chevron" size={10} /></span>
            {/if}
        {/each}
        <!--
            A keyboard-accessible filler that covers the empty trailing space:
            clicking it (or Enter/Space when focused) flips to the editable input,
            the same affordance as the App's Ctrl+L.
        -->
        <button
            type="button"
            class="filler"
            aria-label="Edit path"
            onclick={startEditing}
        ></button>
    {/if}
</div>

<style>
    .crumbs {
        flex: 1;
        min-width: 0;
        display: flex;
        align-items: center;
        gap: 2px;
        background: var(--color-bg-alt);
        border: 1px solid var(--color-border);
        border-radius: 8px;
        height: 30px;
        padding: 0 8px;
        font-size: 12px;
        overflow: hidden;
        cursor: text;
    }
    .seg {
        padding: 3px 6px;
        border-radius: 5px;
        color: var(--color-fg-alt);
        cursor: pointer;
        white-space: nowrap;
        background: transparent;
        border: none;
        font: inherit;
    }
    .seg:hover {
        background: var(--color-surface-hover);
        color: var(--color-fg);
    }
    .seg.last {
        color: var(--color-fg);
        font-weight: 600;
    }
    .sep {
        display: inline-flex;
        align-items: center;
        color: var(--color-muted);
    }
    .filler {
        flex: 1;
        min-width: 0;
        align-self: stretch;
        background: transparent;
        border: none;
        padding: 0;
        cursor: text;
    }
    .path-input {
        flex: 1;
        min-width: 0;
        background: transparent;
        border: none;
        outline: none;
        color: var(--color-fg);
        font-family: var(--font-mono);
        font-size: 12px;
    }
</style>
