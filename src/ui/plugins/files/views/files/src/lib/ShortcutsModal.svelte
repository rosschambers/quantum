<script lang="ts">
    /**
     * The keyboard-shortcuts cheat sheet. It follows the shared overlay house
     * style (a card on a blurred backdrop, no explicit close button), like
     * ConfirmModal, and renders the canonical SHORTCUT_GROUPS from shortcuts.ts
     * so it can never drift from the context-menu accelerators. Escape or a
     * backdrop click closes it; the App owns the open/closed state.
     */
    import { SHORTCUT_GROUPS } from './shortcuts';

    interface Props {
        onClose: () => void;
    }

    const { onClose }: Props = $props();

    let backdrop = $state<HTMLDivElement | null>(null);

    // Focus the backdrop on open so an internal Escape reaches this handler even
    // before the pointer has entered the card.
    $effect(() => {
        backdrop?.focus();
    });

    function handleKeydown(event: KeyboardEvent): void {
        // Keep the App's global key handling from also acting while the sheet is
        // open.
        event.stopPropagation();
        if (event.key === 'Escape') {
            event.preventDefault();
            onClose();
        }
    }

    function handleBackdropClick(event: MouseEvent): void {
        if (event.target === event.currentTarget) {
            onClose();
        }
    }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
<div
    class="backdrop"
    bind:this={backdrop}
    tabindex="-1"
    onclick={handleBackdropClick}
    onkeydown={handleKeydown}
>
    <div class="card" role="dialog" aria-label="Keyboard shortcuts">
        <h3>Keyboard shortcuts</h3>
        <div class="scroll">
            {#each SHORTCUT_GROUPS as group (group.title)}
                <section class="group">
                    <h4>{group.title}</h4>
                    {#each group.hints as hint (hint.description)}
                        <div class="hint" data-hint-row>
                            <span class="description">{hint.description}</span>
                            <kbd class="keys">{hint.keys}</kbd>
                        </div>
                    {/each}
                </section>
            {/each}
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
        outline: none;
    }
    .card {
        width: min(420px, 92vw);
        max-height: 80vh;
        overflow: hidden;
        display: flex;
        flex-direction: column;
        gap: 12px;
        padding: 18px;
        background: var(--color-bg-alt);
        border: 1px solid var(--color-border);
        border-radius: 12px;
        box-shadow: 0 14px 40px var(--color-shadow);
        font-family: var(--font-sans);
    }
    h3 {
        font-size: 14px;
        margin: 0;
        color: var(--color-fg);
    }
    .scroll {
        overflow-y: auto;
        flex: 1;
        display: flex;
        flex-direction: column;
        gap: 14px;
    }
    .group {
        display: flex;
        flex-direction: column;
        gap: 4px;
    }
    h4 {
        font-size: 11px;
        text-transform: uppercase;
        letter-spacing: 0.05em;
        margin: 0 0 2px;
        color: var(--color-muted);
    }
    .hint {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 12px;
        padding: 4px 0;
        border-bottom: 1px solid var(--color-divider);
    }
    .hint:last-child {
        border-bottom: none;
    }
    .description {
        font-size: 12px;
        color: var(--color-fg-alt);
    }
    .keys {
        font-family: var(--font-sans);
        font-size: 11px;
        color: var(--color-muted);
        white-space: nowrap;
    }
</style>
