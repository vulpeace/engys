<script lang="ts">
	import type { Snippet } from 'svelte';

	/** A settings-style row: label + description on the left, control on the right. */
	let {
		label,
		description,
		id,
		stacked = false,
		children
	}: {
		label: string;
		description?: string;
		/** id of the control, so clicking the label focuses it. */
		id?: string;
		/** Put the control below the label instead of beside it. */
		stacked?: boolean;
		children: Snippet;
	} = $props();
</script>

<div class="field" class:stacked>
	<div class="text">
		<label for={id}>{label}</label>
		{#if description}<p>{description}</p>{/if}
	</div>
	<div class="control">{@render children()}</div>
</div>

<style>
	.field {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: var(--space-5);
		padding: var(--space-3) 0;
	}
	.field + :global(.field) {
		border-top: 1px solid var(--color-border);
	}
	.stacked {
		flex-direction: column;
		align-items: stretch;
		gap: var(--space-2);
	}
	label {
		font-weight: var(--weight-medium);
	}
	p {
		font-size: var(--text-xs);
		color: var(--color-text-muted);
	}
	.control {
		display: flex;
		align-items: center;
		gap: var(--space-2);
		flex-shrink: 0;
	}
	.stacked .control {
		flex-shrink: 1;
	}
</style>
