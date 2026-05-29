<script lang="ts">
	import type { Client } from '@quantum/client';
	import type { SystemPowerState } from '../types';
	import { SYSTEM_POWER_CHANNEL } from '../channels';

	interface Props {
		client: Client;
	}

	let { client }: Props = $props();
	let state: SystemPowerState = $state({
		can_shutdown: false,
		can_restart: false,
		can_suspend: false,
		can_hibernate: false,
		can_lock: false,
	});

	let open = $state(false);
	/** Which button is currently armed for confirmation. `null` when none. */
	let armed: 'shutdown' | 'restart' | 'suspend' | 'hibernate' | 'lock' | null = $state(null);
	let disarmTimeout: ReturnType<typeof setTimeout> | null = null;
	let popover: HTMLElement | undefined = $state(undefined);
	let trigger: HTMLElement | undefined = $state(undefined);

	const CONFIRM_WINDOW_MS = 3000;

	$effect(() => {
		const off = client.subscribe(SYSTEM_POWER_CHANNEL, (payload: unknown) => {
			state = payload as SystemPowerState;
		});
		return () => off?.();
	});

	$effect(() => {
		if (!open) return;
		// Close on outside click.
		const onDocumentMousedown = (event: MouseEvent) => {
			const target = event.target as Node | null;
			if (popover && target && popover.contains(target)) return;
			if (trigger && target && trigger.contains(target)) return;
			closeMenu();
		};
		document.addEventListener('mousedown', onDocumentMousedown);
		return () => document.removeEventListener('mousedown', onDocumentMousedown);
	});

	function anyCapable(s: SystemPowerState): boolean {
		return s.can_shutdown || s.can_restart || s.can_suspend || s.can_hibernate || s.can_lock;
	}

	function toggleMenu(): void {
		if (open) {
			closeMenu();
		} else {
			open = true;
		}
	}

	function closeMenu(): void {
		open = false;
		clearArm();
	}

	function clearArm(): void {
		if (disarmTimeout !== null) {
			clearTimeout(disarmTimeout);
			disarmTimeout = null;
		}
		armed = null;
	}

	async function tryAction(command: 'shutdown' | 'restart' | 'suspend' | 'hibernate' | 'lock'): Promise<void> {
		if (armed !== command) {
			// First click: arm. Schedule disarm.
			clearArm();
			armed = command;
			disarmTimeout = setTimeout(() => {
				if (armed === command) armed = null;
				disarmTimeout = null;
			}, CONFIRM_WINDOW_MS);
			return;
		}
		// Second click: confirm + dispatch.
		clearArm();
		try {
			await client.call('action.invoke', {
				provider: 'system_power',
				action: {
					kind: 'custom',
					data: { kind: 'system_power', payload: { command } },
				},
			});
		} catch (err) {
			console.error(`system_power ${command} failed:`, err);
		}
		closeMenu();
	}

	function labelFor(cmd: 'shutdown' | 'restart' | 'suspend' | 'hibernate' | 'lock'): string {
		const base: Record<typeof cmd, string> = {
			shutdown: 'Shutdown',
			restart: 'Restart',
			suspend: 'Suspend',
			hibernate: 'Hibernate',
			lock: 'Lock',
		};
		return armed === cmd ? `Confirm ${base[cmd]}` : base[cmd];
	}
</script>

{#if anyCapable(state)}
	<div class="power-menu-root">
		<button
			type="button"
			class="tray-icon power-menu-trigger"
			aria-label="Power menu"
			bind:this={trigger}
			onclick={toggleMenu}
		>
			⏻
		</button>
		{#if open}
			<div class="power-menu-popover" bind:this={popover} role="menu">
				{#if state.can_shutdown}
					<button
						type="button"
						class:armed={armed === 'shutdown'}
						onclick={() => tryAction('shutdown')}
					>
						{labelFor('shutdown')}
					</button>
				{/if}
				{#if state.can_restart}
					<button
						type="button"
						class:armed={armed === 'restart'}
						onclick={() => tryAction('restart')}
					>
						{labelFor('restart')}
					</button>
				{/if}
				{#if state.can_suspend}
					<button
						type="button"
						class:armed={armed === 'suspend'}
						onclick={() => tryAction('suspend')}
					>
						{labelFor('suspend')}
					</button>
				{/if}
				{#if state.can_hibernate}
					<button
						type="button"
						class:armed={armed === 'hibernate'}
						onclick={() => tryAction('hibernate')}
					>
						{labelFor('hibernate')}
					</button>
				{/if}
				{#if state.can_lock}
					<button type="button" class:armed={armed === 'lock'} onclick={() => tryAction('lock')}>
						{labelFor('lock')}
					</button>
				{/if}
			</div>
		{/if}
	</div>
{/if}

<style>
	.power-menu-root {
		position: relative;
	}
	.tray-icon {
		font-size: var(--tray-icon-size, 14px);
		color: var(--tray-icon-color, var(--color-fg, #cdd6f4));
		background: transparent;
		border: none;
		padding: 0 4px;
		cursor: pointer;
		user-select: none;
	}
	.power-menu-popover {
		position: absolute;
		right: 0;
		top: 100%;
		margin-top: 4px;
		display: flex;
		flex-direction: column;
		gap: 2px;
		background: var(--color-bg-alt, #313244);
		border: 1px solid var(--color-border, #45475a);
		border-radius: var(--radius-md, 4px);
		padding: 4px;
		z-index: 1000;
		min-width: 120px;
	}
	.power-menu-popover button {
		background: transparent;
		color: var(--color-fg, #cdd6f4);
		border: none;
		padding: 4px 8px;
		text-align: left;
		cursor: pointer;
		border-radius: var(--radius-sm, 2px);
	}
	.power-menu-popover button:hover {
		background: var(--color-bg, #1e1e2e);
	}
	.power-menu-popover button.armed {
		color: var(--tray-icon-color-warning, #f38ba8);
		font-weight: bold;
	}
</style>
