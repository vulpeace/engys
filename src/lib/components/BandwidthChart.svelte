<script lang="ts">
	import uPlot from 'uplot';
	import 'uplot/dist/uPlot.min.css';
	import { onMount } from 'svelte';
	import { speed } from '#lib/format.ts';

	let { t, up, down }: { t: number[]; up: number[]; down: number[] } = $props();

	let el: HTMLDivElement;
	let plot: uPlot | null = null;

	const css = (name: string) => getComputedStyle(document.documentElement).getPropertyValue(name).trim();

	function build() {
		plot?.destroy();
		const muted = css('--color-text-muted');
		const grid = css('--color-border');
		const axis = { stroke: muted, grid: { stroke: grid, width: 1 }, ticks: { show: false } };
		plot = new uPlot(
			{
				width: el.clientWidth,
				height: el.clientHeight,
				legend: { show: false },
				cursor: { show: false },
				scales: { y: { range: (_u, _min, max) => [0, Math.max(max * 1.15, 1024)] } },
				axes: [
					{ ...axis, values: (_u, ticks) => ticks.map((v) => new Date(v * 1000).toLocaleTimeString([], { hour12: false })) },
					{ ...axis, size: 70, values: (_u, ticks) => ticks.map((v) => speed(v)) }
				],
				series: [
					{},
					{ label: 'Download', stroke: css('--color-download'), width: 2, fill: css('--color-download') + '22', points: { show: false } },
					{ label: 'Upload', stroke: css('--color-upload'), width: 2, fill: css('--color-upload') + '22', points: { show: false } }
				]
			},
			[t, down, up],
			el
		);
	}

	onMount(() => {
		build();
		const ro = new ResizeObserver(() => plot?.setSize({ width: el.clientWidth, height: el.clientHeight }));
		ro.observe(el);
		// Rebuild when the theme flips so axis/series colors follow the tokens.
		const mq = window.matchMedia('(prefers-color-scheme: dark)');
		const mo = new MutationObserver(build);
		mo.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] });
		mq.addEventListener('change', build);
		return () => {
			ro.disconnect();
			mo.disconnect();
			mq.removeEventListener('change', build);
			plot?.destroy();
		};
	});

	$effect(() => {
		plot?.setData([t, down, up]);
	});
</script>

<div class="chart" bind:this={el}></div>

<style>
	.chart {
		width: 100%;
		height: 100%;
		min-height: 160px;
	}
</style>
