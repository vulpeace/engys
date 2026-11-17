<script lang="ts">
	import { toast } from './toast.svelte.ts';
</script>

<div class="toaster" aria-live="polite">
	{#each toast.items as t (t.id)}
		<button class="toast {t.kind}" onclick={() => toast.dismiss(t.id)}>{t.message}</button>
	{/each}
</div>

<style>
	.toaster {
		position: fixed;
		right: var(--space-4);
		bottom: var(--space-4);
		z-index: 60;
		display: flex;
		flex-direction: column;
		gap: var(--space-2);
		max-width: 420px;
	}
	.toast {
		padding: var(--space-3) var(--space-4);
		background: var(--color-surface);
		border: 1px solid var(--color-border);
		border-left: 3px solid var(--color-accent);
		border-radius: var(--radius-md);
		box-shadow: var(--shadow-overlay);
		font-size: var(--text-sm);
		text-align: left;
		white-space: pre-wrap;
		cursor: pointer;
	}
	.success {
		border-left-color: var(--color-success);
	}
	.error {
		border-left-color: var(--color-danger);
	}
</style>
