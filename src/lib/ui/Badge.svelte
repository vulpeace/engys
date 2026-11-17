<script lang="ts">
	import type { Snippet } from 'svelte';

	let {
		tone = 'neutral',
		children
	}: { tone?: 'neutral' | 'accent' | 'success' | 'warning' | 'danger'; children: Snippet } = $props();
</script>

<span class="badge {tone}">{@render children()}</span>

<style>
	.badge {
		display: inline-flex;
		align-items: center;
		gap: var(--space-1);
		height: 20px;
		padding: 0 var(--space-2);
		border-radius: var(--radius-full);
		font-size: var(--text-xs);
		font-weight: var(--weight-medium);
		white-space: nowrap;
		background: var(--color-surface-2);
		color: var(--color-text-muted);
	}
	.accent {
		background: var(--color-accent-soft);
		color: var(--color-accent);
	}
	.success {
		color: var(--color-success);
	}
	.warning {
		color: var(--color-warning);
	}
	.danger {
		background: var(--color-danger-soft);
		color: var(--color-danger);
	}
</style>
