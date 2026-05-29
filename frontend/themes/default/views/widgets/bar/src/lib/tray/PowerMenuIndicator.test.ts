import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte/svelte5';
import { tick } from 'svelte';
import PowerMenuIndicator from './PowerMenuIndicator.svelte';
import type { SystemPowerState } from '../types';

function mockClient(): {
	client: any;
	emit: (p: SystemPowerState) => Promise<void>;
} {
	let saved: ((p: unknown) => void) | undefined;
	const subscribe = vi.fn((_ch: string, cb: (p: unknown) => void) => {
		saved = cb;
		return () => {};
	});
	const call = vi.fn().mockResolvedValue(undefined);
	const client = { call, subscribe, close: vi.fn() };
	const emit = async (p: SystemPowerState) => {
		await tick();
		saved?.(p);
		await tick();
	};
	return { client, emit };
}

describe('PowerMenuIndicator', () => {
	it('renders nothing when no capabilities', () => {
		const { client } = mockClient();
		const { container } = render(PowerMenuIndicator, { props: { client } });
		expect(container.querySelector('.power-menu-trigger')).toBeNull();
	});

	it('renders trigger when at least one capability is true', async () => {
		const { client, emit } = mockClient();
		const { container } = render(PowerMenuIndicator, { props: { client } });
		await emit({
			can_shutdown: false,
			can_restart: false,
			can_suspend: true,
			can_hibernate: false,
			can_lock: false,
		});
		expect(container.querySelector('.power-menu-trigger')).not.toBeNull();
	});

	it('does not show popover until trigger is clicked', async () => {
		const { client, emit } = mockClient();
		const { container } = render(PowerMenuIndicator, { props: { client } });
		await emit({
			can_shutdown: true,
			can_restart: true,
			can_suspend: true,
			can_hibernate: true,
			can_lock: true,
		});
		expect(container.querySelector('.power-menu-popover')).toBeNull();
	});

	it('shows popover after click and gates buttons by capability', async () => {
		const { client, emit } = mockClient();
		const { container } = render(PowerMenuIndicator, { props: { client } });
		await emit({
			can_shutdown: true,
			can_restart: false,
			can_suspend: true,
			can_hibernate: false,
			can_lock: true,
		});
		await fireEvent.click(container.querySelector('.power-menu-trigger')!);
		await tick();
		const popover = container.querySelector('.power-menu-popover');
		expect(popover).not.toBeNull();
		const labels = Array.from(popover!.querySelectorAll('button')).map(
			(b) => b.textContent?.trim() ?? ''
		);
		expect(labels).toContain('Shutdown');
		expect(labels).toContain('Suspend');
		expect(labels).toContain('Lock');
		expect(labels).not.toContain('Restart');
		expect(labels).not.toContain('Hibernate');
	});

	it('first click on shutdown arms but does not dispatch', async () => {
		const { client, emit } = mockClient();
		const { container } = render(PowerMenuIndicator, { props: { client } });
		await emit({
			can_shutdown: true,
			can_restart: false,
			can_suspend: false,
			can_hibernate: false,
			can_lock: false,
		});
		await fireEvent.click(container.querySelector('.power-menu-trigger')!);
		await tick();
		const shutdownButton = Array.from(
			container.querySelectorAll('.power-menu-popover button')
		).find((b) => b.textContent?.includes('Shutdown'))!;
		await fireEvent.click(shutdownButton);
		await tick();
		expect(client.call).not.toHaveBeenCalledWith('action.invoke', expect.anything());
		expect((shutdownButton as HTMLElement).classList.contains('armed')).toBe(true);
		expect(shutdownButton.textContent).toContain('Confirm Shutdown');
	});

	it('second click on shutdown dispatches the action.invoke envelope', async () => {
		const { client, emit } = mockClient();
		const { container } = render(PowerMenuIndicator, { props: { client } });
		await emit({
			can_shutdown: true,
			can_restart: false,
			can_suspend: false,
			can_hibernate: false,
			can_lock: false,
		});
		await fireEvent.click(container.querySelector('.power-menu-trigger')!);
		await tick();
		const shutdownButton = Array.from(
			container.querySelectorAll('.power-menu-popover button')
		).find((b) => b.textContent?.includes('Shutdown'))!;
		await fireEvent.click(shutdownButton); // arm
		await tick();
		await fireEvent.click(shutdownButton); // confirm
		await tick();
		expect(client.call).toHaveBeenCalledWith('action.invoke', {
			provider: 'system_power',
			action: {
				kind: 'custom',
				data: { kind: 'system_power', payload: { command: 'shutdown' } },
			},
		});
	});

	it('disarms after 3 seconds without confirm', async () => {
		vi.useFakeTimers();
		const { client, emit } = mockClient();
		const { container } = render(PowerMenuIndicator, { props: { client } });
		await emit({
			can_shutdown: true,
			can_restart: false,
			can_suspend: false,
			can_hibernate: false,
			can_lock: false,
		});
		await fireEvent.click(container.querySelector('.power-menu-trigger')!);
		await tick();
		const shutdownButton = Array.from(
			container.querySelectorAll('.power-menu-popover button')
		).find((b) => b.textContent?.includes('Shutdown'))!;
		await fireEvent.click(shutdownButton);
		await tick();
		expect((shutdownButton as HTMLElement).classList.contains('armed')).toBe(true);
		vi.advanceTimersByTime(3500);
		await tick();
		expect((shutdownButton as HTMLElement).classList.contains('armed')).toBe(false);
		expect(client.call).not.toHaveBeenCalledWith('action.invoke', expect.anything());
		vi.useRealTimers();
	});
});
