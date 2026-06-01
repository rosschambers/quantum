<script lang="ts">
    import { createClient } from '@quantum/client';
    import ActiveWindow from './lib/ActiveWindow.svelte';
    import SystemMeters from './lib/SystemMeters.svelte';
    import MediaControls from './lib/MediaControls.svelte';
    import BatteryIndicator from './lib/tray/BatteryIndicator.svelte';
    import BluetoothIndicator from './lib/tray/BluetoothIndicator.svelte';
    import BrightnessIndicator from './lib/tray/BrightnessIndicator.svelte';
    import NetworkIndicator from './lib/tray/NetworkIndicator.svelte';
    import PowerMenuIndicator from './lib/tray/PowerMenuIndicator.svelte';
    import PowerProfileIndicator from './lib/tray/PowerProfileIndicator.svelte';
    import VolumeIndicator from './lib/tray/VolumeIndicator.svelte';

    const client = createClient();

    // Tear down the client on unmount. Subscriptions are owned by the
    // sub-components and clean up themselves through their $effect returns.
    $effect(() => {
        return () => client.close();
    });
</script>

<div class="bar">
    <div class="region region-left">
        <ActiveWindow {client} />
    </div>
    <div class="region region-center">
        <MediaControls {client} />
        <VolumeIndicator {client} />
    </div>
    <div class="region region-right">
        <SystemMeters {client} />
        <BrightnessIndicator {client} />
        <NetworkIndicator {client} />
        <BluetoothIndicator {client} />
        <BatteryIndicator {client} />
        <PowerProfileIndicator {client} />
        <PowerMenuIndicator {client} />
    </div>
</div>

<style>
    .region {
        display: flex;
        align-items: center;
        gap: var(--space-3, 0.75rem);
        min-width: 0;
    }
    .region-left {
        flex: 1;
        justify-content: flex-start;
        overflow: hidden;
    }
    .region-center {
        flex: 0 1 auto;
        justify-content: center;
    }
    .region-right {
        flex: 1;
        justify-content: flex-end;
    }
</style>
