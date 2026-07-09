<script lang="ts">
    import type { PairingRequest } from './types';

    interface Props {
        request: PairingRequest;
        onRespond: (accept: boolean, passkey: number | null, pin: string | null) => void;
    }

    const { request, onRespond }: Props = $props();

    let entry = $state('');

    const needsEntry = $derived(
        request.request === 'request_pin' || request.request === 'request_passkey',
    );

    function submitEntry(): void {
        if (request.request === 'request_pin') {
            onRespond(true, null, entry);
        } else {
            const passkey = Number.parseInt(entry, 10);
            if (Number.isNaN(passkey)) return;
            onRespond(true, passkey, null);
        }
    }

    function titleFor(): string {
        switch (request.request) {
            case 'confirm':
                return 'Confirm pairing code';
            case 'request_pin':
                return 'Enter PIN';
            case 'request_passkey':
                return 'Enter passkey';
            case 'display_passkey':
                return 'Type this passkey on the device';
            case 'authorize_service':
                return 'Allow service connection?';
        }
    }
</script>

<div class="pairing-dialog" role="alertdialog" aria-label="Pairing request">
    <div class="pairing-title">{titleFor()}</div>
    <div class="pairing-device">{request.address}</div>

    {#if request.request === 'confirm' || request.request === 'display_passkey'}
        {#if request.passkey !== null}
            <div class="passkey-digits">
                {String(request.passkey).padStart(6, '0')}
            </div>
        {/if}
    {/if}
    {#if request.request === 'authorize_service' && request.service_uuid !== null}
        <div class="pairing-device">{request.service_uuid}</div>
    {/if}

    {#if needsEntry}
        <input
            type="text"
            bind:value={entry}
            inputmode={request.request === 'request_passkey' ? 'numeric' : 'text'}
            onkeydown={(event) => {
                if (event.key === 'Enter') submitEntry();
            }}
        />
    {/if}

    <div class="pairing-actions">
        {#if request.request === 'confirm'}
            <button type="button" class="btn" onclick={() => onRespond(true, null, null)}>
                Confirm
            </button>
            <button type="button" class="btn-quiet" onclick={() => onRespond(false, null, null)}>
                Cancel
            </button>
        {:else if needsEntry}
            <button type="button" class="btn" onclick={submitEntry}>Pair</button>
            <button type="button" class="btn-quiet" onclick={() => onRespond(false, null, null)}>
                Cancel
            </button>
        {:else if request.request === 'authorize_service'}
            <button type="button" class="btn" onclick={() => onRespond(true, null, null)}>
                Allow
            </button>
            <button type="button" class="btn-quiet" onclick={() => onRespond(false, null, null)}>
                Deny
            </button>
        {:else}
            <button type="button" class="btn-quiet" onclick={() => onRespond(false, null, null)}>
                Cancel
            </button>
        {/if}
    </div>
</div>

<style>
    .pairing-dialog {
        margin: 10px;
        padding: 16px;
        border-radius: 10px;
        border: 1px solid var(--color-accent, #89b4fa);
        background: var(--color-bg, #1e1e2e);
        display: flex;
        flex-direction: column;
        align-items: center;
        gap: 10px;
    }
    .pairing-title {
        font-size: 13px;
        font-weight: 600;
    }
    .pairing-device {
        font-size: 11px;
        color: var(--color-fg-alt, #a6adc8);
    }
    .passkey-digits {
        font-size: 28px;
        font-weight: 700;
        letter-spacing: 0.2em;
        font-variant-numeric: tabular-nums;
    }
    input {
        background: var(--color-bg-alt, #313244);
        color: var(--color-fg, #cdd6f4);
        border: 1px solid var(--color-border, #45475a);
        border-radius: 6px;
        padding: 6px 10px;
        font-size: 13px;
        text-align: center;
        font-family: inherit;
        width: 140px;
    }
    .pairing-actions {
        display: flex;
        gap: 10px;
    }
    .btn {
        background: var(--color-accent, #89b4fa);
        color: var(--color-bg, #1e1e2e);
        border: none;
        border-radius: 7px;
        padding: 7px 14px;
        font-size: 12px;
        font-weight: 600;
        cursor: pointer;
        font-family: inherit;
    }
    .btn-quiet {
        background: none;
        color: var(--color-fg-alt, #a6adc8);
        border: 1px solid var(--color-border, #45475a);
        border-radius: 7px;
        padding: 7px 14px;
        font-size: 12px;
        cursor: pointer;
        font-family: inherit;
    }
</style>
