<script lang="ts">
  interface Props {
    uri: string;
  }

  const { uri }: Props = $props();
</script>

<div class="container">
  <!-- svelte-ignore a11y_media_has_caption -->
  <video src={uri} controls></video>
</div>

<style>
  .container {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100%;
    padding: 32px;
    background: var(--color-bg-alt);
  }

  video {
    max-width: 100%;
    max-height: 100%;
    border-radius: var(--radius-md);
  }
</style>
