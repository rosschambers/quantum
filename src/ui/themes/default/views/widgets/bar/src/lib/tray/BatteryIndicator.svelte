<script lang="ts">
    /**
     * Battery + power-profile combined indicator.
     *
     * The ring around the icon shows the battery's percentage; the
     * glyph centered inside the ring is the active power-profile icon
     * (leaf / balanced / bolt), so a single button surfaces both
     * pieces of state. Clicking the button opens the
     * `widgets/power-profile-menu` view, mirroring the menu-launching
     * pattern used by `PowerMenuIndicator`.
     *
     * When the battery is charging, a small lightning-bolt overlay
     * sits in the bottom-right corner of the icon. That bolt is
     * drawn directly here (a small standalone SVG positioned over
     * the Ring) rather than swapping the centered icon, because the
     * centered glyph already carries profile information that the
     * user still needs to read while charging.
     *
     * Why ignore `state.available`? On many laptops the upower
     * provider reports `available: false` briefly during D-Bus
     * reconnect even while the battery percentage is still known.
     * We render whenever a percentage is present so the indicator
     * does not flicker in and out.
     */
    import type { Client } from '@quantum/client';
    import type { PowerState, PowerProfileState } from '../types';
    import {
        POWER_CHANNEL,
        POWER_PROVIDER,
        POWER_PROFILE_CHANNEL,
        POWER_PROFILE_PROVIDER,
    } from '../channels';
    import { inverseGradientColor } from '../gradient';
    import { powerProfileIcon } from '../icons';
    import Ring from '../Ring.svelte';
    import BarButton from '../BarButton.svelte';

    interface Props {
        client: Client;
    }

    let { client }: Props = $props();

    let battery: PowerState = $state({
        available: false,
        on_battery: false,
        percentage: null,
        state: null,
        time_to_empty_secs: null,
        time_to_full_secs: null,
    });

    let profile: PowerProfileState = $state({
        available: false,
        active: null,
        profiles: [],
        performance_inhibited: null,
    });

    $effect(() => {
        client
            .call('provider.query', { id: POWER_PROVIDER })
            .then((r: unknown) => {
                if (r) battery = r as PowerState;
            })
            .catch(() => {});
        const offBattery = client.subscribe(POWER_CHANNEL, (payload: unknown) => {
            battery = payload as PowerState;
        });
        client
            .call('provider.query', { id: POWER_PROFILE_PROVIDER })
            .then((r: unknown) => {
                if (r) profile = r as PowerProfileState;
            })
            .catch(() => {});
        const offProfile = client.subscribe(POWER_PROFILE_CHANNEL, (payload: unknown) => {
            profile = payload as PowerProfileState;
        });
        return () => {
            offBattery?.();
            offProfile?.();
        };
    });

    async function openProfileMenu(): Promise<void> {
        // Append the per-monitor suffix so the menu opens on the bar's
        // own monitor (see `window.__quantum_monitor` injection in
        // `crates/ui/src/windows/widget.rs`). Same pattern as
        // PowerMenuIndicator and the former PowerProfileIndicator.
        const monitor = window.__quantum_monitor;
        const name = monitor
            ? `widgets/power-profile-menu@${monitor}`
            : 'widgets/power-profile-menu';
        try {
            await client.call('view.show', { name });
        } catch (err) {
            console.error(`view.show ${name} failed:`, err);
        }
    }

    function profileLabel(active: PowerProfileState['active']): string {
        if (active === 'power_saver') return 'Power saver';
        if (active === 'performance') return 'Performance';
        if (active === 'balanced') return 'Balanced';
        return 'Unknown profile';
    }

    function formatTime(totalSecs: number): string {
        const minutes = Math.round(totalSecs / 60);
        if (minutes < 60) return `${minutes}m`;
        const h = Math.floor(minutes / 60);
        const m = minutes % 60;
        return `${h}h ${m}m`;
    }

    function batteryFragment(b: PowerState): string {
        // Charging path. Show "%, (Xh Ym to full)" if we know the time,
        // otherwise just "charging".
        if (b.state === 'charging') {
            if (b.percentage !== null && b.time_to_full_secs !== null) {
                return `${b.percentage.toFixed(0)}% (${formatTime(b.time_to_full_secs)} to full)`;
            }
            if (b.percentage !== null) return `${b.percentage.toFixed(0)}% charging`;
            return 'charging';
        }
        // Discharging path.
        if (b.on_battery) {
            if (b.percentage !== null && b.time_to_empty_secs !== null) {
                return `${b.percentage.toFixed(0)}% (${formatTime(b.time_to_empty_secs)} to empty)`;
            }
            if (b.percentage !== null) return `${b.percentage.toFixed(0)}%`;
            return 'on battery';
        }
        // Plugged in but not actively charging (full, or idle).
        if (b.percentage !== null) return `${b.percentage.toFixed(0)}% AC`;
        return 'AC';
    }

    function tooltipFor(b: PowerState, p: PowerProfileState): string {
        return `${profileLabel(p.active)} \u00b7 ${batteryFragment(b)}`;
    }
</script>

{#if battery.percentage !== null}
    <BarButton
        ariaLabel="Open power profile menu"
        title={tooltipFor(battery, profile)}
        onclick={openProfileMenu}
    >
        <span class="ring-container">
            <Ring
                percent={battery.percentage}
                color={inverseGradientColor(battery.percentage)}
                kind="icon"
                iconName={powerProfileIcon(profile.active)}
            />
            {#if battery.state === 'charging'}
                <!--
                    Charging overlay. Drawn as its own tiny SVG anchored to
                    the bottom-right of the ring container. The bolt path
                    matches the `bolt` icon's polygon, just sized down and
                    tinted with the accent color so it pops against the
                    centered profile glyph.
                -->
                <svg
                    class="charging-bolt"
                    width="9"
                    height="9"
                    viewBox="0 0 24 24"
                    aria-hidden="true"
                >
                    <polygon
                        points="13,2 4,14 11,14 9,22 20,10 13,10 15,2"
                        fill="var(--color-accent, #89b4fa)"
                    />
                </svg>
            {/if}
        </span>
    </BarButton>
{/if}

<style>
    .ring-container {
        position: relative;
        display: inline-flex;
        line-height: 0;
    }
    .charging-bolt {
        position: absolute;
        right: -2px;
        bottom: -2px;
        display: block;
        pointer-events: none;
        /* A subtle stroke around the bolt so it stays readable against
         * a light profile glyph. The drop-shadow filter is cheap on
         * WebKit because the bolt is tiny. */
        filter: drop-shadow(0 0 1px rgba(0, 0, 0, 0.65));
    }
</style>
