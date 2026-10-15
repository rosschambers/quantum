<script lang="ts">
  import type { ClipboardPreview } from './clipboardPreview';

  interface Props {
    preview: ClipboardPreview;
  }

  let { preview }: Props = $props();
  let body: HTMLDivElement | undefined = $state();

  // Page Up / Page Down in the launcher's search input scroll the preview, so
  // a long copy can be read without leaving the keyboard. Moving by most of a
  // page, not all of it, keeps a few lines of context across each jump. The
  // launcher reaches this through `bind:this` on the component instance.
  export function scrollByPage(direction: 1 | -1): void {
    if (body) {
      body.scrollTop += direction * body.clientHeight * 0.9;
    }
  }

  // Start each newly highlighted entry at its top rather than wherever the
  // previous entry was scrolled to.
  $effect(() => {
    void preview;
    if (body) {
      body.scrollTop = 0;
    }
  });
</script>

<div class="clipboard-preview" aria-label="Clipboard entry preview">
  <div class="clipboard-preview-details">{preview.details}</div>
  <div class="clipboard-preview-body" bind:this={body}>
    {#if preview.kind === 'text'}
      <pre class="clipboard-preview-text">{preview.text}</pre>
      {#if preview.hiddenCharacters > 0}
        <div class="clipboard-preview-truncated">
          &hellip; {preview.hiddenCharacters.toLocaleString()} more characters not shown
        </div>
      {/if}
    {:else if preview.kind === 'image'}
      <img class="clipboard-preview-image" src={preview.source} alt="" />
    {/if}
  </div>
</div>
