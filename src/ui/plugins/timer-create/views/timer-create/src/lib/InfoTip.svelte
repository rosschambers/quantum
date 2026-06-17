<script lang="ts">
    /**
     * A small circled question mark that reveals a bubble of explanatory text on
     * hover or keyboard focus. The `text` prop holds the real explanation; the
     * bubble is always in the DOM (so it is testable) and is shown via CSS on
     * `:hover`/`:focus-within`.
     */
    let { text }: { text: string } = $props();
</script>

<span class="info-tip">
    <button
        type="button"
        class="info-tip-mark"
        aria-label={text}
        title={text}
    >?</button>
    <span class="info-tip-bubble" role="tooltip">{text}</span>
</span>

<style>
    .info-tip {
        position: relative;
        display: inline-flex;
    }
    .info-tip-mark {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 15px;
        height: 15px;
        border-radius: 50%;
        border: 1px solid var(--color-fg-muted, #8b94a7);
        background: transparent;
        color: var(--color-fg-muted, #8b94a7);
        font-size: 10px;
        line-height: 1;
        padding: 0;
        cursor: help;
    }
    .info-tip-mark:hover,
    .info-tip-mark:focus-visible {
        border-color: var(--color-accent, #5b9dff);
        color: var(--color-accent, #5b9dff);
        outline: none;
    }
    .info-tip-bubble {
        position: absolute;
        bottom: 140%;
        left: 50%;
        transform: translateX(-50%);
        width: 210px;
        background: var(--color-bg, #0a0e16);
        border: 1px solid var(--color-border, #2a3142);
        color: var(--color-fg, #e6e9ef);
        font-size: 11px;
        font-weight: 400;
        line-height: 1.4;
        text-align: left;
        padding: 8px 10px;
        border-radius: 8px;
        opacity: 0;
        pointer-events: none;
        transition: opacity 0.12s;
        z-index: 10;
        box-shadow: 0 8px 24px var(--color-shadow, rgba(0, 0, 0, 0.5));
    }
    .info-tip:hover .info-tip-bubble,
    .info-tip:focus-within .info-tip-bubble {
        opacity: 1;
    }
</style>
