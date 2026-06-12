<script lang="ts">
	import type { ScheduledJob } from './types';

	interface Props {
		jobs: ScheduledJob[];
		/** Called with the job id when the user clicks the cancel ×. */
		onCancel: (id: string) => Promise<void> | void;
	}

	let { jobs, onCancel }: Props = $props();

	/**
	 * Render "in N min" relative to wall clock. Uses Date.now() at
	 * render time — the list doesn't auto-update its countdown; that
	 * would require a setInterval and the panel typically opens for
	 * less than a minute at a time. The user can close + reopen the
	 * menu to refresh.
	 */
	function relative(fires_at: ScheduledJob['fires_at']): string {
		const fires_ms = fires_at.secs_since_epoch * 1000 + Math.floor(fires_at.nanos_since_epoch / 1e6);
		const delta_ms = fires_ms - Date.now();
		const minutes = Math.max(0, Math.round(delta_ms / 60_000));
		if (minutes === 0) return 'imminent';
		if (minutes === 1) return 'in 1 min';
		if (minutes < 60) return `in ${minutes} min`;
		const hours = Math.floor(minutes / 60);
		const remainingMin = minutes % 60;
		if (remainingMin === 0) return hours === 1 ? 'in 1 hour' : `in ${hours} hours`;
		return `in ${hours}h ${remainingMin}m`;
	}
</script>

{#if jobs.length > 0}
	<div class="scheduled-list" aria-label="Scheduled actions">
		<div class="header">Scheduled</div>
		{#each jobs as job (job.id)}
			<div class="job">
				<span class="label">{job.label}</span>
				<span class="relative">{relative(job.fires_at)}</span>
				<button
					type="button"
					class="cancel"
					aria-label="Cancel {job.label}"
					data-cancel-id={job.id}
					onclick={() => onCancel(job.id)}
				>×</button>
			</div>
		{/each}
	</div>
{/if}

<style>
	.scheduled-list {
		margin-top: 12px;
		padding-top: 12px;
		border-top: 1px solid var(--color-border, #45475a);
	}
	.header {
		font-size: 10px;
		text-transform: uppercase;
		letter-spacing: 0.08em;
		color: var(--color-fg-alt, #a6adc8);
		font-weight: 600;
		margin-bottom: 6px;
	}
	.job {
		display: flex;
		align-items: center;
		gap: 12px;
		padding: 6px 8px;
		font-size: 13px;
	}
	.label {
		flex: 0 0 auto;
	}
	.relative {
		flex: 1;
		color: var(--color-fg-alt, #a6adc8);
		font-size: 12px;
	}
	.cancel {
		background: transparent;
		border: 1px solid transparent;
		color: var(--color-fg-alt, #a6adc8);
		font-size: 16px;
		line-height: 1;
		padding: 2px 8px;
		border-radius: 4px;
		cursor: pointer;
	}
	.cancel:hover {
		background: var(--color-bg, #1e1e2e);
		color: var(--color-fg, #cdd6f4);
		border-color: var(--color-border, #45475a);
	}
</style>
