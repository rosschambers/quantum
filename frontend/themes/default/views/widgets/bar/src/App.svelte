<script lang="ts">
    import { createClient } from '@quantum/client';
    import ActiveWindow from './lib/ActiveWindow.svelte';
    import SystemMeters from './lib/SystemMeters.svelte';
    import MediaControls from './lib/MediaControls.svelte';

    const client = createClient();

    // Tear down the client on unmount. Subscriptions are owned by the
    // sub-components and clean up themselves through their $effect returns.
    $effect(() => {
        return () => client.close();
    });
</script>

<div class="bar">
    <ActiveWindow {client} />
    <SystemMeters {client} />
    <MediaControls {client} />
</div>
