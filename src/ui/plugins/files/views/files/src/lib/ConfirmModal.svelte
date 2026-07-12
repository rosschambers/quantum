<script lang="ts">
    /**
     * A confirmation modal for destructive actions (for example permanent
     * deletion). It follows the shared overlay house style (card on a blurred
     * backdrop, no explicit close button), mirroring PromptModal but without an
     * input: a message and a destructive confirm button. Enter confirms, Escape
     * or a backdrop click cancels. The App owns the open/closed state and decides
     * what confirming does; this component only reports the choice.
     */

    interface Props {
        title: string;
        message: string;
        confirmLabel: string;
        onConfirm: () => void;
        onCancel: () => void;
    }

    const { title, message, confirmLabel, onConfirm, onCancel }: Props = $props();

    let confirmButton = $state<HTMLButtonElement | null>(null);

    // Focus the confirm button on open so Enter confirms immediately.
    $effect(() => {
        confirmButton?.focus();
    });

    function handleKeydown(event: KeyboardEvent): void {
        // Keep the App's global key handling from also acting while the modal is
        // open.
        event.stopPropagation();
        if (event.key === 'Enter') {
            event.preventDefault();
            onConfirm();
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
<div class="backdrop" onclick={handleBackdropClick} onkeydown={handleKeydown}>
    <div class="card confirm-modal" role="dialog" aria-label={title}>
        <h3>{title}</h3>
        <p class="message">{message}</p>
        <div class="actions">
            <button type="button" class="btn ghost" onclick={onCancel}>Cancel</button>
            <button
                type="button"
                class="btn danger"
                bind:this={confirmButton}
                onclick={onConfirm}>{confirmLabel}</button
            >
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
    .message {
        font-size: 12px;
        color: var(--color-fg-alt);
        margin: 0;
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
        background: var(--color-surface-hover, hsla(230, 14%, 42%, 1));
    }
    .btn.danger {
        background: var(--color-error);
        color: var(--color-bg);
        border-color: var(--color-error);
    }
    .btn.danger:hover {
        filter: brightness(1.1);
    }
</style>
