<script lang="ts">
    import type { ActiveWifi } from './types';

    /**
     * Read-only connection details for the active network. Requests a
     * detail fetch once per distinct network, then renders whatever the
     * live state currently carries. The parent reassigns `state`
     * wholesale on every stream tick, so `active` is a fresh object each
     * push; guarding on the ssid string keeps repeated pushes for the
     * same network from re-requesting details.
     */
    interface Props {
        active: ActiveWifi;
        onBack: () => void;
        onFetch: (ssid: string) => void;
    }

    const { active, onBack, onFetch }: Props = $props();

    let lastFetchedSsid: string | null = $state(null);

    $effect(() => {
        if (active.ssid !== lastFetchedSsid) {
            lastFetchedSsid = active.ssid;
            onFetch(active.ssid);
        }
    });

    function valueOrDash(value: string | number | null): string {
        return value === null || value === '' ? '—' : String(value);
    }

    const details = $derived(active.details);
</script>

<div class="detail-head">
    <button type="button" class="back" data-action="back" aria-label="Back" onclick={onBack}>
        &#8592;
    </button>
    <span class="title">{active.ssid}</span>
    <span class="badge connected">connected</span>
</div>
<div class="scroll">
    {#if details}
        <div class="kv"><span class="k">IP address</span><span class="v">{valueOrDash(details.ip_address)}</span></div>
        <div class="kv"><span class="k">Gateway</span><span class="v">{valueOrDash(details.gateway)}</span></div>
        <div class="kv"><span class="k">DNS</span><span class="v">{details.dns.length ? details.dns.join(', ') : '—'}</span></div>
        <div class="kv"><span class="k">MAC address</span><span class="v">{valueOrDash(details.mac)}</span></div>
        <div class="kv"><span class="k">Frequency</span><span class="v">{details.frequency_mhz === null ? '—' : `${details.frequency_mhz} MHz`}</span></div>
        <div class="kv"><span class="k">IPv4 method</span><span class="v">{details.ipv4_method}</span></div>
        <div class="kv"><span class="k">Metered</span><span class="v">{details.metered ? 'yes' : 'no'}</span></div>
    {:else}
        <div class="empty">Loading connection details...</div>
    {/if}
</div>

<style>
    .detail-head {
        display: flex;
        align-items: center;
        gap: 10px;
        padding: 12px 14px;
        border-bottom: 1px solid var(--color-border, #45475a);
    }
    .back {
        cursor: pointer;
        color: var(--color-fg-alt, #a6adc8);
        font-size: 16px;
        background: none;
        border: none;
        padding: 0;
        font-family: inherit;
    }
    .title {
        font-size: 13px;
        font-weight: 600;
        flex: 1;
    }
    .badge {
        font-size: 9px;
        text-transform: uppercase;
        letter-spacing: 0.04em;
        padding: 1px 5px;
        border-radius: 4px;
    }
    .badge.connected {
        background: hsla(115, 54%, 76%, 0.16);
        color: var(--color-good, #a6e3a1);
    }
    .scroll {
        overflow-y: auto;
        flex: 1;
        padding: 6px 0;
    }
    .kv {
        display: flex;
        justify-content: space-between;
        padding: 7px 14px;
        font-size: 12px;
    }
    .kv .k {
        color: var(--color-fg-alt, #a6adc8);
    }
    .kv .v {
        font-family: ui-monospace, monospace;
    }
    .empty {
        text-align: center;
        color: var(--color-fg-alt, #a6adc8);
        font-size: 12px;
        padding: 40px 20px;
    }
</style>
