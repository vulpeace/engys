<script lang="ts" generics="T">
	import type { Snippet } from 'svelte';

	/** Fixed-row-height virtual list: renders only rows in view (+ overscan). */
	let {
		items,
		rowHeight,
		key,
		row,
		header,
		empty,
		minWidth,
		overscan = 8
	}: {
		items: T[];
		rowHeight: number;
		key: (item: T) => string;
		row: Snippet<[T, number]>;
		header?: Snippet;
		empty?: Snippet;
		minWidth?: string;
		overscan?: number;
	} = $props();

	let scrollTop = $state(0);
	let viewport = $state(0);
	let headerHeight = $state(0);

	const start = $derived(Math.max(0, Math.floor(scrollTop / rowHeight) - overscan));
	const end = $derived(Math.min(items.length, Math.ceil((scrollTop + Math.max(0, viewport - headerHeight)) / rowHeight) + overscan));
	const visible = $derived(items.slice(start, end));
</script>

<div class="viewport" bind:clientHeight={viewport} onscroll={(e) => (scrollTop = e.currentTarget.scrollTop)}>
	<div style:min-width={minWidth}>
		{#if header}
			<div class="header" bind:clientHeight={headerHeight}>{@render header()}</div>
		{/if}
		<div class="spacer" style:height="{items.length * rowHeight}px">
			<div class="rows" style:transform="translateY({start * rowHeight}px)">
				{#each visible as item, i (key(item))}
					<div class="row" style:height="{rowHeight}px">{@render row(item, start + i)}</div>
				{/each}
			</div>
		</div>
	</div>
	{#if items.length === 0}
		{@render empty?.()}
	{/if}
</div>

<style>
	.viewport {
		height: 100%;
		overflow: auto;
	}
	.header {
		position: sticky;
		top: 0;
		z-index: 1;
		background: var(--color-surface);
	}
	.spacer {
		position: relative;
	}
	.rows {
		position: absolute;
		inset: 0 0 auto 0;
	}
	.row {
		overflow: hidden;
	}
</style>
