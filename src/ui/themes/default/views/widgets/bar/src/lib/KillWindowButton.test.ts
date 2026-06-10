import { describe, it, expect, vi } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte/svelte5';
import { tick } from 'svelte';
import KillWindowButton from './KillWindowButton.svelte';

function mockClient(): { client: any } {
	const call = vi.fn().mockResolvedValue(undefined);
	const subscribe = vi.fn(() => () => {});
	const client = { call, subscribe, close: vi.fn() };
	return { client };
}

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
			provider: 'shell_command',
			action: {
				kind: 'shell',
				data: { command: ['hyprctl', 'kill'], terminal: false },
			},
		});
	});
});
