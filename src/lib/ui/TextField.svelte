<script lang="ts">
	import type { HTMLInputAttributes } from 'svelte/elements';

	let { value = $bindable(), class: className = '', ...rest }: HTMLInputAttributes = $props();
</script>

<input class="input {className}" bind:value spellcheck="false" autocomplete="off" {...rest} />

<style>
	.input {
		width: 100%;
		height: var(--control-height);
		padding: 0 var(--space-3);
		background: var(--color-surface);
		border: 1px solid var(--color-border);
		border-radius: var(--radius-md);
		font-size: var(--text-sm);
	}
	.input::placeholder {
		color: var(--color-text-muted);
	}
	.input:focus {
		outline: none;
		border-color: var(--color-focus);
	}
	.input:disabled {
		opacity: 0.6;
	}
</style>
