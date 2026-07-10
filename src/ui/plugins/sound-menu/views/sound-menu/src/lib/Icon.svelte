<script lang="ts">
    interface Props {
        name: 'speaker' | 'speaker-muted';
    }
    const { name }: Props = $props();
</script>

<svg
    class="icon"
    width="18"
    height="18"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    stroke-width="2"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
>
    <path d="M4 9v6h4l5 4V5L8 9H4z" />
    {#if name === 'speaker-muted'}
        <line x1="16" y1="9" x2="22" y2="15" />
        <line x1="22" y1="9" x2="16" y2="15" />
    {:else}
        <path d="M16 8a5 5 0 0 1 0 8" />
        <path d="M18.5 5.5a9 9 0 0 1 0 13" />
    {/if}
</svg>

<style>
    .icon {
        display: block;
    }
</style>
