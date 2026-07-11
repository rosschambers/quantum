<script lang="ts">
  import type { ShellCaptureResult } from '@quantum/client';

  interface Props {
    result: ShellCaptureResult | null;
    running: boolean;
  }

  let { result, running }: Props = $props();

  // Header label: while the command is in flight it reads "running"; once a
  // result is in it reads "timed out" when the timeout fired, otherwise the
  // exit code.
  let header = $derived(
    result
      ? result.timed_out
        ? 'timed out'
        : `exit ${result.exit_code}`
      : 'running'
  );

  // The body shows the placeholder only once a result is in and both streams
  // are empty; while running there is nothing to say yet.
  let noOutput = $derived(
    result !== null && result.stdout.length === 0 && result.stderr.length === 0
  );
</script>

<div class="command-output" role="region" aria-label="Command output">
  <div class="command-output-header" class:running>
    {#if running && !result}
      <span class="command-output-spinner" aria-hidden="true">&#9696;</span>
      <span class="command-output-status">running&#8230;</span>
    {:else}
      <span class="command-output-status">{header}</span>
    {/if}
  </div>

  <div class="command-output-body">
    {#if result}
      {#if noOutput}
        <div class="command-output-empty">(no output)</div>
      {:else}
        {#if result.stdout.length > 0}
          <pre class="command-output-stdout">{result.stdout}</pre>
        {/if}
        {#if result.stderr.length > 0}
          <pre class="command-output-stderr">{result.stderr}</pre>
        {/if}
      {/if}
    {/if}
  </div>
</div>
