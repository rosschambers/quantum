<script lang="ts">
    import { createClient } from '@quantum/client';

    /** Bare canonical view name; the registry strips any `@<monitor>` suffix
     * for single-instance overlays, so `view.hide` uses the bare name. */
    const VIEW_NAME = 'plugin/timer-create/timer-create';

    const client = createClient();

    $effect(() => {
        document.addEventListener('keydown', onKeyDown);
        return () => {
            document.removeEventListener('keydown', onKeyDown);
            client.close();
        };
    });

    function onKeyDown(event: KeyboardEvent): void {
        if (event.key === 'Escape') {
            event.preventDefault();
            close();
        }
    }

    function onBackdropClick(event: MouseEvent): void {
        if (event.target === event.currentTarget) close();
    }

    function close(): void {
        client.call('view.hide', { name: VIEW_NAME }).catch(() => {});
    }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
<div class="backdrop" onclick={onBackdropClick}>
    <div class="card" role="dialog" aria-label="New timer">
        <h2 class="title">New timer</h2>
    </div>
</div>

<style>
    .backdrop {
        position: fixed;
        inset: 0;
        background: var(--color-overlay-backdrop, rgba(0, 0, 0, 0.5));
        backdrop-filter: blur(4px);
        display: flex;
        align-items: center;
        justify-content: center;
    }
    .card {
        background: var(--color-bg-alt, #313244);
        color: var(--color-fg, #cdd6f4);
        border-radius: 12px;
        padding: 18px 20px;
        width: 420px;
        box-shadow: 0 14px 40px var(--color-shadow, rgba(0, 0, 0, 0.6));
        border: 1px solid var(--color-border, #45475a);
    }
    .title {
        margin: 0 0 12px;
        font-size: 1.1rem;
    }
</style>
