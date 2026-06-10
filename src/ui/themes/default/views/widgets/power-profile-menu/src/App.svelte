<script lang="ts">
    import { createClient } from '@quantum/client';
    import Icon, { type IconName } from './lib/Icon.svelte';

    type PowerProfile = 'power_saver' | 'balanced' | 'performance';

    interface PowerProfileState {
        available: boolean;
        active: PowerProfile | null;
        profiles: PowerProfile[];
        performance_inhibited: string | null;
    }

    const client = createClient();

    let state: PowerProfileState = $state({
        available: false,
        active: null,
        profiles: [],
        performance_inhibited: null,
    });

    /**
     * Capability + active-profile snapshot, plus live updates.
     */
    $effect(() => {
        client
            .call('provider.query', { id: 'power_profile' })
            .then((r: unknown) => {
                if (r) state = r as PowerProfileState;
            })
            .catch(() => {});
        const off = client.subscribe('power_profile.event', (p: unknown) => {
            state = p as PowerProfileState;
        });
        return () => {
            off?.();
            client.close();
        };
    });

    $effect(() => {
        document.addEventListener('keydown', onKeyDown);
        return () => document.removeEventListener('keydown', onKeyDown);
    });

    function onKeyDown(event: KeyboardEvent): void {
        if (event.key === 'Escape') {
            event.preventDefault();
            close();
        }
    }

    function onBackdropClick(event: MouseEvent): void {
        if (event.target === event.currentTarget) close();
    }

    function close(): void {
        client.call('view.hide', { name: 'widgets/power-profile-menu' }).catch(() => {});
    }

    /**
     * Send the set-profile action, then close immediately. The user
     * picked their profile; no second-click confirmation. The bar's
     * power-profile indicator will reflect the new active profile via
     * the subscribe path after the daemon publishes the next event.
     */
    async function selectProfile(profile: PowerProfile): Promise<void> {
        if (profile === state.active) {
            // Already on this profile; just close.
            close();
            return;
        }
        try {
            await client.call('action.invoke', {
                provider: 'power_profile',
                action: {
                    kind: 'custom',
                    data: {
                        kind: 'power_profile',
                        payload: { command: 'set', profile },
                    },
                },
            });
        } catch (err) {
            console.error(`power_profile set ${profile} failed:`, err);
        }
        close();
    }

    interface ProfileDef {
        profile: PowerProfile;
        label: string;
        description: string;
        icon: IconName;
        /** True when the daemon offers this profile (in state.profiles)
         *  AND it isn't currently inhibited (performance only). */
        available: boolean;
        /** True when this profile is the active one. */
        active: boolean;
        /** Tooltip text for inhibited profiles. */
        inhibitedBy: string | null;
    }

    const profileDefs: ProfileDef[] = $derived([
        {
            profile: 'power_saver',
            label: 'Power Saver',
            description: 'Lower power consumption, quieter operation',
            icon: 'leaf',
            available: state.profiles.includes('power_saver'),
            active: state.active === 'power_saver',
            inhibitedBy: null,
        },
        {
            profile: 'balanced',
            label: 'Balanced',
            description: 'Default balance of performance and efficiency',
            icon: 'balanced',
            available: state.profiles.includes('balanced'),
            active: state.active === 'balanced',
            inhibitedBy: null,
        },
        {
            profile: 'performance',
            label: 'Performance',
            description: 'Maximum performance, higher power draw',
            icon: 'bolt',
            available:
                state.profiles.includes('performance') &&
                state.performance_inhibited === null,
            active: state.active === 'performance',
            inhibitedBy: state.performance_inhibited,
        },
    ]);
</script>

<!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
<div class="backdrop" onclick={onBackdropClick}>
    <div class="card" role="dialog" aria-label="Power profile menu">
        <h2 class="title">Power Profile</h2>
        <div class="tiles">
            {#each profileDefs as def (def.profile)}
                <button
                    type="button"
                    class="profile-tile"
                    class:active={def.active}
                    class:inhibited={def.inhibitedBy !== null}
                    disabled={!def.available && !def.active}
                    onclick={() => selectProfile(def.profile)}
                    data-profile={def.profile}
                    title={def.inhibitedBy
                        ? `Inhibited by ${def.inhibitedBy}`
                        : def.description}
                >
                    <span class="icon">
                        <Icon name={def.icon} size={28} />
                    </span>
                    <span class="label-block">
                        <span class="label">{def.label}</span>
                        <span class="description">
                            {def.inhibitedBy
                                ? `Inhibited: ${def.inhibitedBy}`
                                : def.description}
                        </span>
                    </span>
                    {#if def.active}
                        <span class="active-marker" aria-hidden="true">●</span>
                    {/if}
                </button>
            {/each}
        </div>
    </div>
</div>

<style>
    .backdrop {
        position: fixed;
        inset: 0;
        background: rgba(0, 0, 0, 0.5);
        backdrop-filter: blur(4px);
        display: flex;
        align-items: center;
        justify-content: center;
    }
    .card {
        background: var(--color-bg-alt, #313244);
        color: var(--color-fg, #cdd6f4);
        border-radius: 12px;
        padding: 18px 20px;
        width: 380px;
        box-shadow: 0 14px 40px rgba(0, 0, 0, 0.6);
        border: 1px solid var(--color-border, #45475a);
    }
    .title {
        margin: 0 0 14px;
        font-size: 14px;
        font-weight: 600;
        color: var(--color-fg-alt, #a6adc8);
        letter-spacing: 0.04em;
    }
    .tiles {
        display: flex;
        flex-direction: column;
        gap: 8px;
    }
    .profile-tile {
        display: flex;
        align-items: center;
        gap: 14px;
        padding: 12px 14px;
        background: rgba(0, 0, 0, 0.15);
        color: var(--color-fg, #cdd6f4);
        border: 1px solid var(--color-border, #45475a);
        border-radius: 10px;
        cursor: pointer;
        font-family: inherit;
        font-size: inherit;
        text-align: left;
        transition: background-color 0.12s ease, border-color 0.12s ease,
            color 0.12s ease;
    }
    .profile-tile:hover:not(:disabled) {
        background: hsla(230, 14%, 32%, 0.9);
        border-color: var(--color-accent, #89b4fa);
    }
    .profile-tile.active {
        background: hsla(217, 92%, 76%, 0.12);
        border-color: var(--color-accent, #89b4fa);
        color: var(--color-accent, #89b4fa);
    }
    .profile-tile.inhibited {
        opacity: 0.55;
    }
    .profile-tile:disabled {
        opacity: 0.45;
        cursor: not-allowed;
    }
    .icon {
        display: inline-flex;
        flex-shrink: 0;
    }
    .label-block {
        display: flex;
        flex-direction: column;
        flex: 1;
        gap: 2px;
        min-width: 0;
    }
    .label {
        font-size: 14px;
        font-weight: 600;
        line-height: 1.2;
    }
    .description {
        font-size: 11px;
        color: var(--color-fg-alt, #a6adc8);
        line-height: 1.3;
    }
    .profile-tile.active .description {
        color: var(--color-accent, #89b4fa);
        opacity: 0.7;
    }
    .active-marker {
        flex-shrink: 0;
        font-size: 12px;
        color: var(--color-accent, #89b4fa);
    }
</style>
