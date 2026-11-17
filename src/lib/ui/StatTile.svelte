<script lang="ts">
	import type { Snippet } from 'svelte';

	let {
		label,
		value = '',
		values,
		description,
		hint,
		tone = 'default',
		icon
	}: {
		label: string;
		value?: string;
		values?: { label: string; value: string }[];
		description?: string;
		hint?: string;
		tone?: 'default' | 'upload' | 'download';
		icon?: Snippet;
	} = $props();
</script>

<div class="tile {tone}" title={description}>
	<div class="label">
		{#if icon}<span class="icon">{@render icon()}</span>{/if}
		{label}
	</div>
	{#if values}
		<div class="split-values">
			{#each values as metric (metric.label)}
				<div class="metric"><span class="metric-label">{metric.label}</span><span class="metric-value num" title={metric.value}>{metric.value}</span></div>
			{/each}
		</div>
	{:else}<div class="value num">{value}</div>{/if}
	{#if hint}<div class="hint">{hint}</div>{/if}
</div>

<style>
	.split-values { display: grid; grid-auto-flow: column; grid-auto-columns: minmax(0, 1fr); gap: var(--space-2); }
	.metric { display: flex; flex-direction: column; gap: var(--space-1); min-width: 0; }
	.metric + .metric { border-left: 1px solid var(--color-border); padding-left: var(--space-2); }
	.metric-label { font-size: var(--text-xs); color: var(--color-text-muted); }
	.metric-value { font-size: var(--text-lg); font-weight: var(--weight-bold); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
	.tile {
		display: flex;
		flex-direction: column;
		gap: var(--space-1);
		padding: var(--space-4);
		background: var(--color-surface);
		border: 1px solid var(--color-border);
		border-radius: var(--radius-lg);
		min-width: 0;
	}
	.label {
		display: flex;
		align-items: center;
		gap: var(--space-2);
		font-size: var(--text-sm);
		color: var(--color-text-muted);
	}
	.icon {
		display: inline-flex;
	}
	.value {
		font-size: var(--text-2xl);
		font-weight: var(--weight-bold);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.hint {
		font-size: var(--text-xs);
		color: var(--color-text-muted);
	}
	.upload .icon {
		color: var(--color-upload);
	}
	.download .icon {
		color: var(--color-download);
	}
</style>
