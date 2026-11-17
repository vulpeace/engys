<script lang="ts">
	import { Switch } from 'bits-ui';

	let {
		checked = $bindable(false),
		disabled = false,
		label,
		onCheckedChange
	}: {
		checked?: boolean;
		disabled?: boolean;
		/** Accessible label when there's no visible <label>. */
		label?: string;
		onCheckedChange?: (v: boolean) => void;
	} = $props();
</script>

<!-- Bits UI renders the elements, so styles target them through :global() + a ui-* class. -->
<Switch.Root bind:checked {disabled} {onCheckedChange} aria-label={label} class="ui-switch">
	<Switch.Thumb class="ui-switch-thumb" />
</Switch.Root>

<style>
	:global(.ui-switch) {
		position: relative;
		display: inline-flex;
		align-items: center;
		flex-shrink: 0;
		width: 36px;
		height: 20px;
		padding: 2px;
		border: none;
		border-radius: var(--radius-full);
		background: var(--color-border);
		cursor: pointer;
		transition: background var(--duration-fast) var(--ease);
	}
	:global(.ui-switch[data-state='checked']) {
		background: var(--color-accent);
	}
	:global(.ui-switch[data-disabled]) {
		opacity: 0.5;
		cursor: default;
	}
	:global(.ui-switch-thumb) {
		display: block;
		width: 16px;
		height: 16px;
		border-radius: var(--radius-full);
		background: #fff;
		box-shadow: 0 1px 2px rgb(0 0 0 / 0.25);
		transition: transform var(--duration-fast) var(--ease);
	}
	:global(.ui-switch-thumb[data-state='checked']) {
		transform: translateX(16px);
	}
</style>
