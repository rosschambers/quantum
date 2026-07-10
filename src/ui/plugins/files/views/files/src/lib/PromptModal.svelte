<script lang="ts">
    /**
     * A single-field name prompt modal, reused for renaming an entry and for
     * naming a new folder or file. It follows the shared overlay house style
     * (card on a blurred backdrop, no explicit close button). Enter submits the
     * trimmed value, Escape or a backdrop click cancels. The App owns the
     * open/closed state and decides what to do with the submitted name; this
     * component only collects it.
     */
    import { untrack } from 'svelte';

    interface Props {
        title: string;
        initial: string;
        onSubmit: (value: string) => void;
        onCancel: () => void;
    }

    const { title, initial, onSubmit, onCancel }: Props = $props();

    // Seed the field once from the initial name; the prop never changes for a
    // given modal instance, so capturing only the initial value is intended.
    let value = $state(untrack(() => initial));
    let inputElement = $state<HTMLInputElement | null>(null);

    // Focus and select the field on open so the first keystroke replaces it.
    $effect(() => {
        const element = inputElement;
        if (element !== null) {
            element.focus();
            element.select();
        }
    });

    function submit(): void {
        const trimmed = value.trim();
        if (trimmed !== '') {
            onSubmit(trimmed);
        }
    }

    function handleKeydown(event: KeyboardEvent): void {
        // Keep the App's global key handling (type-ahead, Escape-closes) from
        // also acting while the field is focused.
        event.stopPropagation();
        if (event.key === 'Enter') {
            event.preventDefault();
            submit();
        } else if (event.key === 'Escape') {
            event.preventDefault();
            onCancel();
        }
    }

    function handleBackdropClick(event: MouseEvent): void {
        if (event.target === event.currentTarget) {
            onCancel();
        }
    }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
<div class="backdrop" onclick={handleBackdropClick}>
    <div class="card prompt-modal" role="dialog" aria-label={title}>
        <h3>{title}</h3>
        <input
            class="name-input"
            bind:this={inputElement}
            bind:value
            onkeydown={handleKeydown}
            spellcheck="false"
            autocomplete="off"
        />
        <div class="actions">
            <button type="button" class="btn ghost" onclick={onCancel}>Cancel</button>
            <button type="button" class="btn primary" onclick={submit}>OK</button>
        </div>
    </div>
</div>

<style>
    .backdrop {
        position: fixed;
        inset: 0;
        z-index: 300;
        display: flex;
        align-items: center;
        justify-content: center;
        background: var(--color-overlay-backdrop, rgba(0, 0, 0, 0.5));
        backdrop-filter: blur(4px);
    }
    .card {
        width: min(420px, 92vw);
        display: flex;
        flex-direction: column;
        gap: 12px;
        padding: 18px;
        background: var(--color-bg-alt);
        border: 1px solid var(--color-border);
        border-radius: 12px;
        box-shadow: 0 14px 40px var(--color-shadow);
    }
    h3 {
        font-size: 14px;
        margin: 0;
        color: var(--color-fg);
    }
    .name-input {
        background: var(--color-bg);
        border: 1px solid var(--color-border);
        border-radius: 8px;
        padding: 8px 10px;
        color: var(--color-fg);
        font-family: var(--font-mono);
        font-size: 12px;
        outline: none;
    }
    .name-input:focus {
        border-color: var(--color-accent);
    }
    .actions {
        display: flex;
        justify-content: flex-end;
        gap: 8px;
    }
    .btn {
        border-radius: 8px;
        padding: 7px 14px;
        font-size: 12px;
        font-weight: 600;
        cursor: pointer;
        border: 1px solid var(--color-border);
    }
    .btn.ghost {
        background: transparent;
        color: var(--color-fg-alt);
    }
    .btn.ghost:hover {
        background: var(--color-surface-hover);
    }
    .btn.primary {
        background: var(--color-accent);
        color: var(--color-bg);
        border-color: var(--color-accent);
    }
    .btn.primary:hover {
        filter: brightness(1.1);
    }
</style>
