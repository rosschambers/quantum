<script lang="ts">
    import type { Client } from '@quantum/client';
    import NetworkIndicator from './tray/NetworkIndicator.svelte';
    import BluetoothIndicator from './tray/BluetoothIndicator.svelte';
    import VolumeIndicator from './tray/VolumeIndicator.svelte';
    import BrightnessIndicator from './tray/BrightnessIndicator.svelte';
    import BatteryIndicator from './tray/BatteryIndicator.svelte';
    import PowerProfileIndicator from './tray/PowerProfileIndicator.svelte';
    import PowerMenuIndicator from './tray/PowerMenuIndicator.svelte';

    interface Props {
        client: Client;
    }

    let { client }: Props = $props();
</script>

<div class="tray">
    <NetworkIndicator {client} />
    <BluetoothIndicator {client} />
    <VolumeIndicator {client} />
    <BrightnessIndicator {client} />
    <BatteryIndicator {client} />
    <PowerProfileIndicator {client} />
    <PowerMenuIndicator {client} />
</div>

<style>
    .tray {
        display: flex;
        align-items: center;
        gap: var(--tray-gap, 8px);
    }
</style>
