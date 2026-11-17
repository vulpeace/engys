<script lang="ts">
	import { ToggleGroup } from 'bits-ui';

	/** A single-choice segmented control (e.g. Rule / Global / Direct). */
	let {
		value = $bindable(''),
		options,
		disabled = false,
		label,
		onValueChange
	}: {
		value?: string;
		options: { value: string; label: string }[];
		disabled?: boolean;
		label?: string;
		onValueChange?: (v: string) => void;
	} = $props();
</script>

<ToggleGroup.Root
	type="single"
	{value}
	{disabled}
	aria-label={label}
	class="ui-segmented"
	onValueChange={(v) => {
		// Single toggle groups allow deselecting; a segmented control shouldn't.
		if (!v) return;
		value = v;
		onValueChange?.(v);
	}}
>
	{#each options as opt (opt.value)}
		<ToggleGroup.Item value={opt.value} class="ui-segmented-item">{opt.label}</ToggleGroup.Item>
	{/each}
</ToggleGroup.Root>

<style>
	:global(.ui-segmented) {
		display: inline-flex;
		padding: 2px;
		gap: 2px;
		background: var(--color-surface-2);
		border-radius: var(--radius-md);
	}
	:global(.ui-segmented-item) {
		height: calc(var(--control-height) - 4px);
		padding: 0 var(--space-3);
		border: none;
		border-radius: var(--radius-sm);
		background: transparent;
		font-size: var(--text-sm);
		color: var(--color-text-muted);
		cursor: pointer;
	}
	:global(.ui-segmented-item[data-state='on']) {
		background: var(--color-surface);
		color: var(--color-text);
		box-shadow: 0 1px 2px rgb(0 0 0 / 0.08);
	}
	:global(.ui-segmented-item[data-disabled]) {
		cursor: default;
		opacity: 0.6;
	}
</style>
