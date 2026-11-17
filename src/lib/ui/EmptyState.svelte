<script lang="ts">
	import type { Snippet } from 'svelte';

	let { title, description, children }: { title: string; description?: string; children?: Snippet } =
		$props();
</script>

<div class="empty">
	<h3>{title}</h3>
	{#if description}<p>{description}</p>{/if}
	{#if children}<div class="actions">{@render children()}</div>{/if}
</div>

<style>
	.empty {
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		gap: var(--space-2);
		padding: var(--space-6) var(--space-4);
		text-align: center;
	}
	h3 {
		font-size: var(--text-lg);
		font-weight: var(--weight-bold);
	}
	p {
		max-width: 420px;
		color: var(--color-text-muted);
	}
	.actions {
		display: flex;
		gap: var(--space-2);
		margin-top: var(--space-2);
	}
</style>
