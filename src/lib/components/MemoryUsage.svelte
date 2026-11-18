<script lang="ts">
	import { onMount } from 'svelte';
	import { MemoryStick } from '@lucide/svelte';
	import { backend, type AndroidMemory } from '#lib/api/backend.ts';
	import { core } from '#lib/stores/core.svelte.ts';
	import { live } from '#lib/stores/live.svelte.ts';
	import { bytes } from '#lib/format.ts';
	import { StatTile } from '#lib/ui/index.ts';

	let clientMemory = $state<number | null>(null);
    let androidMemory = $state<AndroidMemory | null>(null);
	onMount(() => {
		let disposed = false;
		let pending = false;
		async function refresh() {
			if (disposed || pending || document.hidden) return;
			pending = true;
			try {
                if (core.android) {
                    const memory = await backend.androidMemory();
                    if (!disposed) androidMemory = memory;
                } else {
                    const memory = await backend.clientMemory();
                    if (!disposed) clientMemory = memory;
                }
			} catch {
				if (!disposed) { clientMemory = null; androidMemory = null; }
			} finally { pending = false; }
		}
		void refresh();
		const timer = setInterval(refresh, 2000);
		const onVisibility = () => { if (!document.hidden) void refresh(); };
		document.addEventListener('visibilitychange', onVisibility);
		return () => {
			disposed = true;
			clearInterval(timer);
			document.removeEventListener('visibilitychange', onVisibility);
		};
	});
</script>

<StatTile label="Memory" values={core.android ? [
    { label: 'App total', value: bytes(androidMemory?.processRss) },
    { label: 'Core heap', value: bytes(androidMemory?.coreHeap) }
] : [
	{ label: 'Core', value: core.running && live.memory > 0 ? bytes(live.memory) : '—' },
	{ label: 'Client', value: bytes(clientMemory) }
]} hint={core.android ? 'App RSS · Go heap allocated' : 'Resident memory (RSS)'} description={core.android
    ? 'App total sums RSS of the UI and VPN service processes, excluding separate WebView renderers. Shared pages can be counted twice. Core heap is allocated Go objects included in that total; the values are not additive.'
    : 'Core: mihomo. Client: this app and its child processes, including WebView, excluding mihomo. Resident-memory sums can count shared pages more than once.'}>
	{#snippet icon()}<MemoryStick size={16} />{/snippet}
</StatTile>
