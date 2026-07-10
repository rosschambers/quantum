<script lang="ts">
    /**
     * Renders the reactive toast store as a bottom-right stack, matching the
     * playground's `.toast` look. Error toasts are tinted with `--color-error`.
     * The store owns the toast lifecycle (auto-dismiss); this component is a
     * pure view over it.
     */
    import { toasts } from './toasts.svelte';
</script>

<div id="toasts">
    {#each toasts as toast (toast.id)}
        <div class="toast" class:error={toast.kind === 'error'}>{toast.message}</div>
    {/each}
</div>

<style>
    #toasts {
        position: fixed;
        bottom: 24px;
        right: 24px;
        display: flex;
        flex-direction: column;
        gap: 8px;
        z-index: 200;
        pointer-events: none;
    }
    .toast {
        background: var(--color-bg-alt);
        border: 1px solid var(--color-border);
        border-radius: 10px;
        box-shadow: 0 14px 40px var(--color-shadow);
        padding: 9px 14px;
        font-size: 12px;
        color: var(--color-fg);
        font-family: var(--font-sans);
        max-width: 340px;
        animation: toast-in 0.18s ease-out;
    }
    .toast.error {
        border-color: var(--color-error);
        color: var(--color-error);
    }
    @keyframes toast-in {
        from {
            opacity: 0;
            transform: translateY(6px);
        }
    }
</style>
