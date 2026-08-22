<script lang="ts">
  interface Props {
    uri: string;
    filename: string;
  }

  const { uri, filename }: Props = $props();
</script>

<div class="container">
  <img src={uri} alt={filename} />
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

  img {
    object-fit: contain;
    max-width: 100%;
    max-height: 100%;
  }
</style>
