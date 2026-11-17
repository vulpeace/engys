<script lang="ts">
	import { Dialog } from 'bits-ui';
	import { X } from '@lucide/svelte';
	import type { Snippet } from 'svelte';

	let {
		open = $bindable(false),
		title,
		description,
		wide = false,
		footer,
		children
	}: {
		open?: boolean;
		title: string;
		description?: string;
		wide?: boolean;
		footer?: Snippet;
		children?: Snippet;
	} = $props();
</script>

<Dialog.Root bind:open>
	<Dialog.Portal>
		<Dialog.Overlay class="ui-dialog-overlay" />
		<Dialog.Content class="ui-dialog {wide ? 'ui-dialog-wide' : ''}">
			<header>
				<div>
					<Dialog.Title class="ui-dialog-title">{title}</Dialog.Title>
					{#if description}
						<Dialog.Description class="ui-dialog-desc">{description}</Dialog.Description>
					{/if}
				</div>
				<Dialog.Close class="ui-dialog-close" aria-label="Close"><X size={16} /></Dialog.Close>
			</header>
			<div class="body">{@render children?.()}</div>
			{#if footer}<footer>{@render footer()}</footer>{/if}
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>

<style>
	:global(.ui-dialog-overlay) {
		position: fixed;
		inset: 0;
		z-index: 40;
		background: var(--color-scrim);
	}
	:global(.ui-dialog) {
		position: fixed;
		z-index: 41;
		top: 50%;
		left: 50%;
		transform: translate(-50%, -50%);
		display: flex;
		flex-direction: column;
		width: min(480px, calc(100vw - 2 * var(--space-5)));
		max-height: calc(100vh - 2 * var(--space-5));
		background: var(--color-surface);
		border: 1px solid var(--color-border);
		border-radius: var(--radius-lg);
		box-shadow: var(--shadow-overlay);
	}
	:global(.ui-dialog-wide) {
		width: min(900px, calc(100vw - 2 * var(--space-5)));
		height: calc(100vh - 2 * var(--space-6));
	}
	header {
		display: flex;
		justify-content: space-between;
		align-items: flex-start;
		gap: var(--space-3);
		padding: var(--space-4);
		border-bottom: 1px solid var(--color-border);
	}
	:global(.ui-dialog-title) {
		margin: 0;
		font-size: var(--text-lg);
		font-weight: var(--weight-bold);
	}
	:global(.ui-dialog-desc) {
		margin: var(--space-1) 0 0;
		font-size: var(--text-sm);
		color: var(--color-text-muted);
	}
	:global(.ui-dialog-close) {
		display: inline-flex;
		padding: var(--space-1);
		border: none;
		border-radius: var(--radius-sm);
		background: transparent;
		cursor: pointer;
		color: var(--color-text-muted);
	}
	:global(.ui-dialog-close:hover) {
		background: var(--color-surface-2);
	}
	.body {
		flex: 1;
		min-height: 0;
		padding: var(--space-4);
		overflow: auto;
		display: flex;
		flex-direction: column;
		gap: var(--space-3);
	}
	footer {
		display: flex;
		justify-content: flex-end;
		gap: var(--space-2);
		padding: var(--space-3) var(--space-4);
		border-top: 1px solid var(--color-border);
	}
</style>
