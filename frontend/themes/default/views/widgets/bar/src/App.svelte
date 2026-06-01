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
        <div class="group">
            <SystemMeters {client} />
        </div>
        <div class="divider" aria-hidden="true"></div>
        <div class="group">
            <BrightnessIndicator {client} />
            <NetworkIndicator {client} />
            <BluetoothIndicator {client} />
            <BatteryIndicator {client} />
        </div>
        <div class="divider" aria-hidden="true"></div>
        <div class="group">
            <PowerProfileIndicator {client} />
            <PowerMenuIndicator {client} />
        </div>
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
        gap: 8px;
    }
    /*
     * Right-region grouping. Each `.group` is a flex row of related
     * indicators (meters, status, power). The `.divider` between
     * groups is a thin vertical line that sits at the bar row's
     * baseline; opacity and color are tuned so it reads as a soft
     * separator rather than competing with the indicators.
     */
    .group {
        display: flex;
        align-items: center;
        gap: 10px;
    }
    .divider {
        width: 1px;
        height: 18px;
        background: var(--color-fg-alt, #a6adc8);
        opacity: 0.35;
    }
</style>
