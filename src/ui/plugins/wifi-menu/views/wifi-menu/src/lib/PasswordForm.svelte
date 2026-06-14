<script lang="ts">
    /**
     * Inline password entry that expands underneath a secured, unsaved
     * network row. Submitting calls back into the parent with the typed
     * password; the parent owns the actual connect command.
     */
    interface Props {
        ssid: string;
        error: 'incorrect_password' | 'error' | null;
        onSubmit: (password: string) => void;
        onCancel: () => void;
    }

    const { ssid, error, onSubmit, onCancel }: Props = $props();

    let password = $state('');
    let showPassword = $state(false);

    function submit(): void {
        onSubmit(password);
    }

    function onKeyDown(event: KeyboardEvent): void {
        if (event.key === 'Enter') {
            event.preventDefault();
            submit();
        }
    }
</script>

<div class="pwform">
    <div class="pw-title">Password for <strong>{ssid}</strong></div>
    <div class="pwfield">
        <!-- svelte-ignore a11y_autofocus -->
        <input
            type={showPassword ? 'text' : 'password'}
            placeholder="Network password"
            aria-label="Network password"
            bind:value={password}
            onkeydown={onKeyDown}
            autofocus
        />
        <button type="button" class="btn" onclick={submit}>Connect</button>
        <button type="button" class="btn ghost" onclick={onCancel}>Cancel</button>
    </div>
    <label class="showpw">
        <input type="checkbox" bind:checked={showPassword} /> Show password
    </label>
    {#if error === 'incorrect_password'}
        <div class="pwerror">Incorrect password. Try again.</div>
    {:else if error === 'error'}
        <div class="pwerror">Connection failed.</div>
    {/if}
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
    .pwfield {
        display: flex;
        gap: 8px;
    }
    .pwfield input[type='text'],
    .pwfield input[type='password'] {
        flex: 1;
        background: var(--color-bg-alt, #313244);
        border: 1px solid var(--color-border, #45475a);
        color: var(--color-fg, #cdd6f4);
        border-radius: 7px;
        padding: 7px 9px;
        font-size: 12px;
        font-family: inherit;
    }
    .pwfield input:focus {
        outline: none;
        border-color: var(--color-accent, #89b4fa);
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
    .pwerror {
        color: var(--color-bad, #f38ba8);
        font-size: 11px;
        margin-top: 8px;
    }
    .showpw {
        font-size: 11px;
        color: var(--color-fg-alt, #a6adc8);
        margin-top: 8px;
        display: flex;
        align-items: center;
        gap: 6px;
        cursor: pointer;
    }
</style>
