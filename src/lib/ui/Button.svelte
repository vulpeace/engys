<script lang="ts">
	import type { Snippet } from 'svelte';
	import type { HTMLButtonAttributes } from 'svelte/elements';

	type Props = HTMLButtonAttributes & {
		variant?: 'primary' | 'secondary' | 'ghost' | 'danger';
		size?: 'sm' | 'md';
		/** Square button for a single icon. */
		icon?: boolean;
		loading?: boolean;
		children?: Snippet;
	};

	let {
		variant = 'secondary',
		size = 'md',
		icon = false,
		loading = false,
		disabled,
		type = 'button',
		children,
		...rest
	}: Props = $props();
</script>

<button
	{type}
	class="btn {variant} {size}"
	class:icon
	class:loading
	disabled={disabled || loading}
	aria-busy={loading}
	{...rest}
>
	{@render children?.()}
</button>

<style>
	.btn {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		gap: var(--space-2);
		height: var(--control-height);
		padding: 0 var(--space-3);
		border: 1px solid transparent;
		border-radius: var(--radius-md);
		font-size: var(--text-sm);
		font-weight: var(--weight-medium);
		white-space: nowrap;
		cursor: pointer;
		transition:
			background var(--duration-fast) var(--ease),
			border-color var(--duration-fast) var(--ease);
	}
	.btn:disabled {
		opacity: 0.5;
		cursor: default;
	}
	.loading {
		cursor: progress;
	}
	.sm {
		height: 26px;
		padding: 0 var(--space-2);
		font-size: var(--text-xs);
	}
	.icon {
		width: var(--control-height);
		padding: 0;
	}
	.icon.sm {
		width: 26px;
	}

	.primary {
		background: var(--color-accent);
		color: var(--color-accent-contrast);
	}
	.primary:hover:not(:disabled) {
		background: var(--color-accent-hover);
	}
	.secondary {
		background: var(--color-surface);
		border-color: var(--color-border);
	}
	.secondary:hover:not(:disabled) {
		background: var(--color-surface-2);
	}
	.ghost {
		background: transparent;
	}
	.ghost:hover:not(:disabled) {
		background: var(--color-surface-2);
	}
	.danger {
		background: var(--color-surface);
		border-color: var(--color-border);
		color: var(--color-danger);
	}
	.danger:hover:not(:disabled) {
		background: var(--color-danger-soft);
	}
</style>
