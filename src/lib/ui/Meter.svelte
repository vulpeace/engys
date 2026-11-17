<script lang="ts">
	/** Horizontal usage bar (e.g. subscription traffic quota). */
	let { value, max, label }: { value: number; max: number; label?: string } = $props();
	const pct = $derived(max > 0 ? Math.min(100, (value / max) * 100) : 0);
	const tone = $derived(pct > 90 ? 'danger' : pct > 75 ? 'warning' : 'ok');
</script>

<div
	class="meter {tone}"
	role="meter"
	aria-valuenow={value}
	aria-valuemin={0}
	aria-valuemax={max}
	aria-label={label}
>
	<div class="fill" style:width="{pct}%"></div>
</div>

<style>
	.meter {
		height: 6px;
		border-radius: var(--radius-full);
		background: var(--color-surface-2);
		overflow: hidden;
	}
	.fill {
		height: 100%;
		background: var(--color-accent);
		border-radius: inherit;
	}
	.warning .fill {
		background: var(--color-warning);
	}
	.danger .fill {
		background: var(--color-danger);
	}
</style>
