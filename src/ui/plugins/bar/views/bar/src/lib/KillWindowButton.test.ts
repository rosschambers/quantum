import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte/svelte5';
import { tick, flushSync } from 'svelte';
import { closeContextMenu } from '@quantum/client';
import KillWindowButton from './KillWindowButton.svelte';
import type { WindowListEntry } from './types';

function mockClient(): { client: any } {
	const call = vi.fn().mockResolvedValue(undefined);
	const subscribe = vi.fn(() => () => {});
	const client = { call, subscribe, close: vi.fn() };
	return { client };
}

function mockClientWithWindows(windows: WindowListEntry[]): { client: any } {
	const call = vi.fn((method: string) => {
		if (method === 'provider.query') {
			return Promise.resolve({ windows });
		}
		return Promise.resolve(undefined);
	});
	const subscribe = vi.fn(() => () => {});
	const client = { call, subscribe, close: vi.fn() };
	return { client };
}

function mockClientQueryFails(): { client: any } {
	const call = vi.fn((method: string) => {
		if (method === 'provider.query') {
			return Promise.reject(new Error('no hyprland'));
		}
		return Promise.resolve(undefined);
	});
	const subscribe = vi.fn(() => () => {});
	const client = { call, subscribe, close: vi.fn() };
	return { client };
}

async function waitForMenu(): Promise<void> {
	for (let attempt = 0; attempt < 20; attempt += 1) {
		flushSync();
		if (document.querySelector('[data-quantum-context-menu]')) return;
		await Promise.resolve();
	}
}

function menuItem(text: string): HTMLButtonElement | undefined {
	return Array.from(
		document.querySelectorAll('[data-quantum-context-menu] [role="menuitem"]'),
	).find((el) => el.textContent?.includes(text)) as HTMLButtonElement | undefined;
}

afterEach(() => {
	closeContextMenu();
	document.body.innerHTML = '';
});

describe('KillWindowButton', () => {
	it('renders a button', () => {
		const { client } = mockClient();
		const { container } = render(KillWindowButton, { props: { client } });
		const btn = container.querySelector('.bar-button');
		expect(btn).not.toBeNull();
	});

	it('click invokes shell_command provider with hyprctl kill', async () => {
		const { client } = mockClient();
		const { container } = render(KillWindowButton, { props: { client } });
		const btn = container.querySelector('.bar-button') as HTMLButtonElement | null;
		expect(btn).not.toBeNull();
		await fireEvent.click(btn!);
		await tick();
		expect(client.call).toHaveBeenCalledWith('action.invoke', {
			provider: 'shell',
			action: {
				kind: 'shell',
				data: { command: ['hyprctl', 'kill'], terminal: false },
			},
		});
	});

	it('right-click lists open windows and closes the selected one by address', async () => {
		const windows: WindowListEntry[] = [
			{
				address: '0xabc',
				class: 'firefox',
				title: 'Mozilla Firefox',
				workspace_id: 1,
				workspace_name: '1',
			},
		];
		const { client } = mockClientWithWindows(windows);
		const { container } = render(KillWindowButton, { props: { client } });
		const btn = container.querySelector('.bar-button') as HTMLButtonElement;
		flushSync();

		btn.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true }));
		await waitForMenu();

		const windowEntry = menuItem('firefox');
		expect(windowEntry).toBeTruthy();
		expect(windowEntry!.textContent).toContain('Mozilla Firefox');

		await fireEvent.click(windowEntry!);
		await tick();

		expect(client.call).toHaveBeenCalledWith('action.invoke', {
			provider: 'shell',
			action: {
				kind: 'shell',
				data: {
					command: ['hyprctl', 'dispatch', 'closewindow', 'address:0xabc'],
					terminal: false,
				},
			},
		});
	});

	it('right-click "Kill active window" dispatches killactive', async () => {
		const { client } = mockClientWithWindows([]);
		const { container } = render(KillWindowButton, { props: { client } });
		const btn = container.querySelector('.bar-button') as HTMLButtonElement;
		flushSync();

		btn.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true }));
		await waitForMenu();

		const killActive = menuItem('Kill active window');
		expect(killActive).toBeTruthy();

		await fireEvent.click(killActive!);
		await tick();

		expect(client.call).toHaveBeenCalledWith('action.invoke', {
			provider: 'shell',
			action: {
				kind: 'shell',
				data: { command: ['hyprctl', 'dispatch', 'killactive'], terminal: false },
			},
		});
	});

	it('shows the static items when the window query fails', async () => {
		const { client } = mockClientQueryFails();
		const { container } = render(KillWindowButton, { props: { client } });
		const btn = container.querySelector('.bar-button') as HTMLButtonElement;
		flushSync();

		btn.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true }));
		await waitForMenu();

		expect(menuItem('Kill active window')).toBeTruthy();
		expect(menuItem('Pick window to kill')).toBeTruthy();
	});

	it('dismissing the menu without selecting performs no kill', async () => {
		const windows: WindowListEntry[] = [
			{
				address: '0xabc',
				class: 'firefox',
				title: 'Mozilla Firefox',
				workspace_id: 1,
				workspace_name: '1',
			},
		];
		const { client } = mockClientWithWindows(windows);
		const { container } = render(KillWindowButton, { props: { client } });
		const btn = container.querySelector('.bar-button') as HTMLButtonElement;
		flushSync();

		btn.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true }));
		await waitForMenu();
		expect(menuItem('firefox')).toBeTruthy();

		// Dismiss the menu without selecting any item.
		closeContextMenu();

		const invokeCalls = client.call.mock.calls.filter(
			(args: unknown[]) => args[0] === 'action.invoke',
		);
		expect(invokeCalls).toHaveLength(0);
	});
});
