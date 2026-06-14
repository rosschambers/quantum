<script lang="ts">
    import Signal from './Signal.svelte';
    import { SECURITY_LABEL, BAND_LABEL } from './labels';
    import type { WifiNetwork } from './types';

    /**
     * A single available-network row: signal bars, ssid (or hidden
     * tag), badges (connected/saved/band), security text, an inline
     * connect status, and a hover-revealed settings cog for saved
     * networks. The parent owns click routing (connect vs expand).
     */
    interface Props {
        network: WifiNetwork;
        status: 'connecting' | 'error' | 'incorrect_password' | null;
        onSelect: () => void;
        onSettings: (() => void) | null;
    }

    const { network, status, onSelect, onSettings }: Props = $props();

    const isHidden = $derived(network.ssid === '');

    function settings(event: MouseEvent): void {
        event.stopPropagation();
        onSettings?.();
    }

    function onRowKeyDown(event: KeyboardEvent): void {
        if (event.key === 'Enter' || event.key === ' ') {
            // Space would otherwise scroll the list.
            event.preventDefault();
            onSelect();
        }
    }
</script>

<div
    class="net"
    class:active={network.active}
    data-bssid={network.bssid}
    role="button"
    tabindex="0"
    onclick={onSelect}
    onkeydown={onRowKeyDown}
>
    <span class="signal"><Signal percent={network.signal_percent} /></span>
    <span class="meta">
        <span class="ssid">
            {#if isHidden}
                <span class="hidden-tag">Hidden network</span>
            {:else}
                {network.ssid}
            {/if}
        </span>
        <span class="subline">
            {#if network.active}
                <span class="badge connected">connected</span>
            {:else if network.saved}
                <span class="badge saved">saved</span>
            {/if}
            <span class="badge">{BAND_LABEL[network.band]} GHz</span>
            <span>{SECURITY_LABEL[network.security]}</span>
        </span>
    </span>
    <span class="right">
        {#if status === 'connecting'}
            <span class="row-status"><span class="spinner"></span> connecting...</span>
        {:else if status === 'incorrect_password'}
            <span class="row-status error">Incorrect password</span>
        {:else if status === 'error'}
            <span class="row-status error">Connection failed</span>
        {/if}
        {#if onSettings}
            <button
                type="button"
                class="cog"
                data-action="settings"
                aria-label="Network settings"
                onclick={settings}
            >
                &#9881;
            </button>
        {/if}
    </span>
</div>

<style>
    .net {
        display: flex;
        align-items: center;
        gap: 12px;
        padding: 12px;
        border-radius: 9px;
        cursor: pointer;
        transition: background 0.12s;
    }
    .net:hover {
        background: rgba(255, 255, 255, 0.04);
    }
    .net.active {
        background: hsla(217, 92%, 76%, 0.12);
    }
    .signal {
        width: 22px;
        text-align: center;
        color: var(--color-fg, #cdd6f4);
        flex-shrink: 0;
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
    .ssid .hidden-tag {
        font-style: italic;
        color: var(--color-fg-alt, #a6adc8);
        font-weight: 400;
    }
    .subline {
        font-size: 11px;
        color: var(--color-fg-alt, #a6adc8);
        display: flex;
        gap: 8px;
        align-items: center;
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
    .badge.connected {
        background: hsla(115, 54%, 76%, 0.16);
        color: var(--color-good, #a6e3a1);
        border-color: transparent;
    }
    .badge.saved {
        color: var(--color-accent, #89b4fa);
    }
    .right {
        display: flex;
        align-items: center;
        gap: 8px;
        flex-shrink: 0;
    }
    .cog {
        color: var(--color-fg-alt, #a6adc8);
        cursor: pointer;
        opacity: 0;
        transition: opacity 0.12s;
        font-size: 13px;
        background: none;
        border: none;
        padding: 0;
        font-family: inherit;
    }
    .net:hover .cog {
        opacity: 1;
    }
    .row-status {
        font-size: 11px;
        color: var(--color-accent, #89b4fa);
        display: flex;
        align-items: center;
        gap: 6px;
    }
    .row-status.error {
        color: var(--color-bad, #f38ba8);
    }
    .spinner {
        width: 12px;
        height: 12px;
        border-radius: 50%;
        border: 2px solid var(--color-border, #45475a);
        border-top-color: var(--color-accent, #89b4fa);
        animation: spin 0.7s linear infinite;
    }
    @keyframes spin {
        to {
            transform: rotate(360deg);
        }
    }
</style>
