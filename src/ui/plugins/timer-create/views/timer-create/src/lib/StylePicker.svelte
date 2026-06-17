<script lang="ts">
    import type { VisualStyle } from '@quantum/client';

    /**
     * Style picker for a new timer. Owns the chosen `style` and `accentHue`
     * locally and emits a partial `VisualConfig` (`{ style, accent_hue }`)
     * through `onChange` whenever either changes. The parent merges the
     * partial over the fetched `defaults_visual` so a complete object is sent.
     */
    let {
        style = 'mixed',
        accentHue = 210,
        onChange,
    }: {
        style?: VisualStyle;
        accentHue?: number;
        onChange: (partial: { style: VisualStyle; accent_hue: number }) => void;
    } = $props();

    const STYLES: VisualStyle[] = ['ring', 'wedge', 'pie', 'dots', 'bar', 'mixed'];

    function selectStyle(next: VisualStyle): void {
        style = next;
        onChange({ style, accent_hue: accentHue });
    }

    function onHueInput(event: Event): void {
        accentHue = Number((event.target as HTMLInputElement).value);
        onChange({ style, accent_hue: accentHue });
    }
</script>

<div class="style-picker">
    <div class="styles" role="group" aria-label="Visual style">
        {#each STYLES as candidate (candidate)}
            <button
                type="button"
                data-style={candidate}
                class:active={style === candidate}
                onclick={() => selectStyle(candidate)}
            >
                {candidate}
            </button>
        {/each}
    </div>

    <label class="hue">
        <span class="hue-label">Accent</span>
        <input
            data-field="accent-hue"
            type="range"
            min="0"
            max="360"
            value={accentHue}
            oninput={onHueInput}
        />
        <span class="hue-value">{accentHue}</span>
    </label>
</div>

<style>
    .style-picker {
        display: flex;
        flex-direction: column;
        gap: 8px;
    }
    .styles {
        display: flex;
        flex-wrap: wrap;
        gap: 6px;
    }
    .styles button {
        flex: 1;
        min-width: 60px;
        padding: 5px 8px;
        border-radius: 6px;
        border: 1px solid var(--color-border, #45475a);
        background: var(--color-bg, #1e1e2e);
        color: var(--color-fg, #cdd6f4);
        cursor: pointer;
        text-transform: capitalize;
    }
    .styles button.active {
        background: var(--color-accent, #89b4fa);
        color: var(--color-bg, #1e1e2e);
        border-color: var(--color-accent, #89b4fa);
    }
    .hue {
        display: flex;
        align-items: center;
        gap: 10px;
    }
    .hue-label {
        width: 80px;
        flex-shrink: 0;
        color: var(--color-fg-muted, #a6adc8);
    }
    .hue input {
        flex: 1;
    }
    .hue-value {
        width: 32px;
        text-align: right;
        color: var(--color-fg-muted, #a6adc8);
    }
</style>
