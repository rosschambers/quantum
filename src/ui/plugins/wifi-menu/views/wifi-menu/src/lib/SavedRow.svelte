<script lang="ts">
    import { SECURITY_LABEL } from './labels';
    import type { SavedNetwork } from './types';

    /**
     * A saved-network row in the Saved tab: ssid, an in-range badge,
     * security and auto-connect state, plus a settings cog (opens the
     * advanced panel) and a Forget button. All actions are callbacks;
     * the parent owns the commands.
     */
    interface Props {
        network: SavedNetwork;
        onSettings: () => void;
        onForget: () => void;
    }

    const { network, onSettings, onForget }: Props = $props();
</script>

<div class="net" data-saved-id={network.id}>
    <span class="meta">
        <span class="ssid">
            {network.ssid}
            {#if network.in_range}
                <span class="badge">in range</span>
            {/if}
        </span>
        <span class="subline">
            <span>{SECURITY_LABEL[network.security]}</span>
            <span>{network.autoconnect ? 'auto-connect on' : 'auto-connect off'}</span>
        </span>
    </span>
    <span class="right">
        <button
            type="button"
            class="btn ghost icon"
            data-action="settings"
            aria-label="Advanced settings"
            onclick={onSettings}
        >
            &#9881;
        </button>
        <button
            type="button"
            class="btn ghost danger"
            data-action="forget"
            onclick={onForget}
        >
            Forget
        </button>
    </span>
</div>

<style>
    .net {
        display: flex;
        align-items: center;
        gap: 12px;
        padding: 12px;
        border-radius: 9px;
    }
    .net:hover {
        background: rgba(255, 255, 255, 0.04);
    }
    .meta {
        flex: 1;
        min-width: 0;
    }
    .ssid {
        font-size: 13px;
        font-weight: 600;
        display: flex;
        align-items: center;
        gap: 7px;
    }
    .subline {
        font-size: 11px;
        color: var(--color-fg-alt, #a6adc8);
        display: flex;
        gap: 8px;
        margin-top: 2px;
    }
    .badge {
        font-size: 9px;
        text-transform: uppercase;
        letter-spacing: 0.04em;
        padding: 1px 5px;
        border-radius: 4px;
        background: var(--color-bg, #1e1e2e);
        color: var(--color-fg-alt, #a6adc8);
        border: 1px solid var(--color-border, #45475a);
    }
    .right {
        display: flex;
        align-items: center;
        gap: 8px;
        flex-shrink: 0;
    }
    .btn {
        background: var(--color-accent, #89b4fa);
        color: var(--color-bg, #1e1e2e);
        border: none;
        border-radius: 7px;
        padding: 4px 9px;
        font-size: 12px;
        font-weight: 600;
        cursor: pointer;
        font-family: inherit;
    }
    .btn.ghost {
        background: transparent;
        color: var(--color-fg-alt, #a6adc8);
        border: 1px solid var(--color-border, #45475a);
        font-weight: 400;
    }
    .btn.icon {
        font-size: 13px;
    }
    .btn.danger {
        color: var(--color-bad, #f38ba8);
        border-color: var(--color-bad, #f38ba8);
    }
</style>
