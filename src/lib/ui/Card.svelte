<script lang="ts">
	import type { Snippet } from 'svelte';

	let {
		title,
		actions,
		padded = true,
		class: className = '',
		children
	}: {
		title?: string;
		/** Rendered on the right side of the header. */
		actions?: Snippet;
		padded?: boolean;
		class?: string;
		children?: Snippet;
	} = $props();
</script>

<section class="card {className}">
	{#if title || actions}
		<header>
			{#if title}<h2>{title}</h2>{/if}
			{#if actions}<div class="actions">{@render actions()}</div>{/if}
		</header>
	{/if}
	<div class="body" class:padded>
		{@render children?.()}
	</div>
</section>

<style>
	.card {
		display: flex;
		flex-direction: column;
		min-width: 0;
		background: var(--color-surface);
		border: 1px solid var(--color-border);
		border-radius: var(--radius-lg);
	}
	header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: var(--space-3);
		padding: var(--space-3) var(--space-4);
		border-bottom: 1px solid var(--color-border);
	}
	h2 {
		font-size: var(--text-md);
		font-weight: var(--weight-bold);
	}
	.actions {
		display: flex;
		gap: var(--space-2);
	}
	.body {
		flex: 1;
		min-height: 0;
	}
	.padded {
		padding: var(--space-4);
	}
</style>
