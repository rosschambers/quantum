<script lang="ts">
    /**
     * The bottom status bar: item count on the left, a selection summary in the
     * middle (only when something is selected), and the active drive's label and
     * free space on the right. The selection size is formatted with the shared
     * `formatSize` helper; the free-space text arrives already formatted so the
     * App can pick the right drive.
     */
    import { formatSize } from './format';

    interface Props {
        /** How many entries are visible in the active pane. */
        itemCount: number;
        /** How many of them are selected. */
        selectionCount: number;
        /** Total bytes across the selection. */
        selectionBytes: number;
        /** The active drive's short label, e.g. "Home". */
        driveLabel: string;
        /** Already-formatted free space, e.g. "82.3 GB". */
        driveFree: string;
    }

    let { itemCount, selectionCount, selectionBytes, driveLabel, driveFree }: Props = $props();

    const selectionSummary = $derived(
        selectionCount > 0
            ? `${selectionCount} selected (${formatSize(selectionBytes)})`
            : '',
    );
</script>

<div class="statusbar">
    <span class="count">{itemCount} items</span>
    <span class="sel">{selectionSummary}</span>
    <span class="free">{driveLabel}: {driveFree} free</span>
</div>

<style>
    .statusbar {
        height: 24px;
        flex: none;
        display: flex;
        align-items: center;
        gap: 16px;
        padding: 0 12px;
        border-top: 1px solid var(--color-border);
        font-size: 10px;
        color: var(--color-muted);
        font-family: var(--font-mono, ui-monospace, monospace);
    }
    .free {
        margin-left: auto;
    }
</style>
