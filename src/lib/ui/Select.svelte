<script lang="ts">
	import { Select } from 'bits-ui';
	import { Check, ChevronDown } from '@lucide/svelte';

	export type Option = { value: string; label: string; disabled?: boolean };

	let {
		value = $bindable(''),
		options,
		placeholder = 'Select…',
		disabled = false,
		label,
		onValueChange
	}: {
		value?: string;
		options: Option[];
		placeholder?: string;
		disabled?: boolean;
		label?: string;
		onValueChange?: (v: string) => void;
	} = $props();

	const selectedLabel = $derived(options.find((o) => o.value === value)?.label);
</script>

<Select.Root type="single" bind:value {disabled} {onValueChange} items={options}>
	<Select.Trigger class="ui-select-trigger" aria-label={label}>
		<span class:placeholder={!selectedLabel}>{selectedLabel ?? placeholder}</span>
		<ChevronDown size={14} />
	</Select.Trigger>
	<Select.Portal>
		<Select.Content class="ui-select-content" sideOffset={4}>
			<Select.Viewport>
				{#each options as opt (opt.value)}
					<Select.Item class="ui-select-item" value={opt.value} label={opt.label} disabled={opt.disabled}>
						{#snippet children({ selected })}
							<span>{opt.label}</span>
							{#if selected}<Check size={14} />{/if}
						{/snippet}
					</Select.Item>
				{/each}
			</Select.Viewport>
		</Select.Content>
	</Select.Portal>
</Select.Root>

<style>
	.placeholder {
		color: var(--color-text-muted);
	}
	:global(.ui-select-trigger) {
		display: inline-flex;
		align-items: center;
		justify-content: space-between;
		gap: var(--space-2);
		min-width: 140px;
		height: var(--control-height);
		padding: 0 var(--space-3);
		background: var(--color-surface);
		border: 1px solid var(--color-border);
		border-radius: var(--radius-md);
		font-size: var(--text-sm);
		cursor: pointer;
	}
	:global(.ui-select-trigger[data-disabled]) {
		opacity: 0.5;
		cursor: default;
	}
	:global(.ui-select-content) {
		z-index: 50;
		min-width: var(--bits-select-anchor-width);
		max-height: var(--bits-select-content-available-height);
		padding: var(--space-1);
		background: var(--color-surface);
		border: 1px solid var(--color-border);
		border-radius: var(--radius-md);
		box-shadow: var(--shadow-overlay);
		overflow-y: auto;
	}
	:global(.ui-select-item) {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: var(--space-3);
		padding: var(--space-1) var(--space-2);
		border-radius: var(--radius-sm);
		font-size: var(--text-sm);
		cursor: pointer;
	}
	:global(.ui-select-item[data-highlighted]) {
		background: var(--color-surface-2);
	}
	:global(.ui-select-item[data-disabled]) {
		opacity: 0.5;
	}
</style>
