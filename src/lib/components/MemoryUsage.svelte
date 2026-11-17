<script lang="ts">
	import { onMount } from 'svelte';
	import { MemoryStick } from '@lucide/svelte';
	import { backend } from '#lib/api/backend.ts';
	import { core } from '#lib/stores/core.svelte.ts';
	import { live } from '#lib/stores/live.svelte.ts';
	import { bytes } from '#lib/format.ts';
	import { StatTile } from '#lib/ui/index.ts';

	let clientMemory = $state<number | null>(null);
	onMount(() => {
		let disposed = false;
		let pending = false;
		async function refresh() {
			if (disposed || pending || document.hidden) return;
			pending = true;
			try {
				const memory = await backend.clientMemory();
				if (!disposed) clientMemory = memory;
			} catch {
				if (!disposed) clientMemory = null;
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

<StatTile label="Memory" values={[
	{ label: 'Core', value: core.running && live.memory > 0 ? bytes(live.memory) : '—' },
	{ label: 'Client', value: bytes(clientMemory) }
]} hint="Resident memory (RSS)" description="Core: mihomo. Client: this app and its child processes, including WebView, excluding mihomo. Resident-memory sums can count shared pages more than once.">
	{#snippet icon()}<MemoryStick size={16} />{/snippet}
</StatTile>
