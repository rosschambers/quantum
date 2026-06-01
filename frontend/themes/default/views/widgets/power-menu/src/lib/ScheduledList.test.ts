import { describe, it, expect, vi } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte/svelte5';
import { tick } from 'svelte';
import ScheduledList from './ScheduledList.svelte';
import type { ScheduledJob } from './types';

function makeJob(overrides: Partial<ScheduledJob> = {}): ScheduledJob {
	return {
		id: 'abc12345',
		fires_at: {
			secs_since_epoch: Math.floor(Date.now() / 1000) + 15 * 60,
			nanos_since_epoch: 0,
		},
		label: 'Suspend',
		...overrides,
	};
}

describe('ScheduledList', () => {
	it('renders nothing when jobs is empty', () => {
		const { container } = render(ScheduledList, {
			props: { jobs: [], onCancel: vi.fn() },
		});
		expect(container.querySelector('.scheduled-list')).toBeNull();
	});

	it('renders one job row per scheduled job', () => {
		const jobs = [
			makeJob({ id: 'aaa11111', label: 'Suspend' }),
			makeJob({ id: 'bbb22222', label: 'Restart' }),
		];
		const { container } = render(ScheduledList, { props: { jobs, onCancel: vi.fn() } });
		const rows = container.querySelectorAll('.job');
		expect(rows).toHaveLength(2);
		const labels = Array.from(rows).map((r) => r.querySelector('.label')?.textContent?.trim());
		expect(labels).toEqual(['Suspend', 'Restart']);
	});

	it('renders the relative time as "in N min"', () => {
		const jobs = [
			makeJob({
				id: 'a',
				label: 'Suspend',
				fires_at: {
					secs_since_epoch: Math.floor(Date.now() / 1000) + 15 * 60,
					nanos_since_epoch: 0,
				},
			}),
		];
		const { container } = render(ScheduledList, { props: { jobs, onCancel: vi.fn() } });
		const rel = container.querySelector('.relative');
		expect(rel?.textContent).toMatch(/in 1[45] min/); // allow for rounding
	});

	it('clicking the cancel x calls onCancel with the job id', async () => {
		const onCancel = vi.fn();
		const jobs = [makeJob({ id: 'targetId' })];
		const { container } = render(ScheduledList, { props: { jobs, onCancel } });
		const cancel = container.querySelector('[data-cancel-id="targetId"]') as HTMLElement;
		await fireEvent.click(cancel);
		await tick();
		expect(onCancel).toHaveBeenCalledWith('targetId');
	});

	it('formats imminent jobs (under 1 min) as "imminent"', () => {
		const jobs = [
			makeJob({
				fires_at: {
					secs_since_epoch: Math.floor(Date.now() / 1000) + 10,
					nanos_since_epoch: 0,
				},
			}),
		];
		const { container } = render(ScheduledList, { props: { jobs, onCancel: vi.fn() } });
		const rel = container.querySelector('.relative');
		expect(rel?.textContent).toBe('imminent');
	});

	it('formats hour-plus durations as "in Nh Mm"', () => {
		const jobs = [
			makeJob({
				fires_at: {
					secs_since_epoch: Math.floor(Date.now() / 1000) + 2 * 3600 + 5 * 60,
					nanos_since_epoch: 0,
				},
			}),
		];
		const { container } = render(ScheduledList, { props: { jobs, onCancel: vi.fn() } });
		const rel = container.querySelector('.relative');
		// 125 min total; formatter says "in 2h 5m"
		expect(rel?.textContent).toMatch(/in 2h [45]m/);
	});
});
