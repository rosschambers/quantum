<script lang="ts">
    /**
     * Hidden-network form reached from the footer. Collects an ssid and
     * an optional password, then calls back to the parent which issues
     * the connect_hidden command.
     */
    interface Props {
        onSubmit: (ssid: string, password: string) => void;
        onCancel: () => void;
    }

    const { onSubmit, onCancel }: Props = $props();

    let ssid = $state('');
    let password = $state('');

    function submit(): void {
        if (ssid.trim() === '') return;
        onSubmit(ssid, password);
    }
</script>

<div class="pwform">
    <div class="pw-title">Connect to a hidden network</div>
    <div class="form">
        <div class="field">
            <label for="hidden-ssid">Network name (SSID)</label>
            <input id="hidden-ssid" type="text" placeholder="SSID" bind:value={ssid} />
        </div>
        <div class="field">
            <label for="hidden-password">Password</label>
            <input
                id="hidden-password"
                type="text"
                placeholder="Password (leave blank if open)"
                bind:value={password}
            />
        </div>
    </div>
    <div class="pwfield">
        <button type="button" class="btn" data-action="connect-hidden" onclick={submit}>
            Connect
        </button>
        <button type="button" class="btn ghost" onclick={onCancel}>Cancel</button>
    </div>
</div>

<style>
    .pwform {
        margin: 4px 6px 10px;
        padding: 12px;
        background: var(--color-bg, #1e1e2e);
        border: 1px solid var(--color-border, #45475a);
        border-radius: 9px;
    }
    .pw-title {
        font-size: 12px;
        margin-bottom: 9px;
        color: var(--color-fg, #cdd6f4);
    }
    .form {
        display: flex;
        flex-direction: column;
        gap: 8px;
    }
    .field {
        display: flex;
        flex-direction: column;
        gap: 4px;
    }
    .field label {
        font-size: 11px;
        color: var(--color-fg-alt, #a6adc8);
    }
    .field input {
        background: var(--color-bg-alt, #313244);
        border: 1px solid var(--color-border, #45475a);
        color: var(--color-fg, #cdd6f4);
        border-radius: 7px;
        padding: 7px 9px;
        font-size: 12px;
        font-family: inherit;
    }
    .field input:focus {
        outline: none;
        border-color: var(--color-accent, #89b4fa);
    }
    .pwfield {
        display: flex;
        gap: 8px;
        margin-top: 10px;
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
    .btn.ghost {
        background: transparent;
        color: var(--color-fg-alt, #a6adc8);
        border: 1px solid var(--color-border, #45475a);
        font-weight: 400;
    }
</style>
