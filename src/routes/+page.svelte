<script lang="ts">
	import { ArrowDown, ArrowUp, Cable, Play, Square } from '@lucide/svelte';
	import { Badge, Button, Card, PageHeader, StatTile, toast } from '#lib/ui/index.ts';
	import MemoryUsage from '#lib/components/MemoryUsage.svelte';
	import BandwidthChart from '#lib/components/BandwidthChart.svelte';
	import TunControl from '#lib/components/TunControl.svelte';
	import { core } from '#lib/stores/core.svelte.ts';
	import { live } from '#lib/stores/live.svelte.ts';
	import { backend, type ProfileIndex } from '#lib/api/backend.ts';
	import { bytes, duration, speed } from '#lib/format.ts';

	let profiles = $state<ProfileIndex | null>(null);
	let version = $state<string | null>(null);
	let now = $state(Date.now());
	let logEl = $state<HTMLPreElement>();

	backend.profilesList().then((p) => (profiles = p));

	$effect(() => {
		const id = setInterval(() => (now = Date.now()), 1000);
		return () => clearInterval(id);
	});

	/** Proxy port as the running core reports it (may come from the profile). */
	let proxyPort = $state<number | null>(null);

	$effect(() => {
		const api = core.api;
		version = null;
		proxyPort = null;
		api?.version().then((v) => (version = v.version)).catch(() => {});
		api
			?.configs()
			.then((c) => {
				const p = Number(c['mixed-port']) || Number(c['port']) || 0;
				proxyPort = p > 0 ? p : null;
			})
			.catch(() => {});
	});

	// Keep the log panel pinned to the bottom.
	$effect(() => {
		void core.logs.length;
		if (logEl) logEl.scrollTop = logEl.scrollHeight;
	});

	const activeProfile = $derived(
		profiles?.profiles.find((p) => p.id === (core.status?.profileId ?? profiles?.active))
	);
	const uptime = $derived(core.status?.startedAt ? duration(now / 1000 - core.status.startedAt) : '—');

	async function toggle(on: boolean) {
		try {
			if (on) await core.start();
			else await core.stop();
		} catch (e) {
			toast.error(e);
		}
	}
</script>

<PageHeader title="Dashboard" subtitle={activeProfile ? `Profile: ${activeProfile.name}` : 'No profile selected'}>
	{#snippet actions()}
		{#if core.running}
			<Badge tone="success">Running · {uptime}</Badge>
		{:else}
			<Badge>Stopped</Badge>
		{/if}
		<Button variant={core.running ? 'secondary' : 'primary'} loading={core.busy} onclick={() => toggle(!core.running)}>
			{#if core.running}<Square size={14} /> Stop{:else}<Play size={14} /> Start{/if}
		</Button>
	{/snippet}
</PageHeader>

{#if core.status?.lastError}
	<Card>
		<pre class="error selectable">{core.status.lastError}</pre>
	</Card>
{/if}

<div class="tiles">
	<StatTile label="Download" value={speed(live.traffic.down)} tone="download" hint="Total {bytes(live.downloadTotal)}">
		{#snippet icon()}<ArrowDown size={16} />{/snippet}
	</StatTile>
	<StatTile label="Upload" value={speed(live.traffic.up)} tone="upload" hint="Total {bytes(live.uploadTotal)}">
		{#snippet icon()}<ArrowUp size={16} />{/snippet}
	</StatTile>
	<MemoryUsage />
	<StatTile
		label="Connections"
		value={String(live.activeCount)}
		hint="Session traffic {bytes(live.uploadTotal + live.downloadTotal)}"
	>
		{#snippet icon()}<Cable size={16} />{/snippet}
	</StatTile>
</div>

<div class="grid">
	<Card title="Bandwidth" class="chart-card">
		{#snippet actions()}
			<span class="legend"><i class="dl"></i>Download <i class="ul"></i>Upload</span>
		{/snippet}
		<BandwidthChart t={live.samples.t} up={live.samples.up} down={live.samples.down} />
	</Card>

	<Card title="Core">
		<dl>
			<dt>Version</dt>
			<dd class="mono">{version ?? '—'}</dd>
			<dt>PID</dt>
			<dd class="mono">{core.status?.pid ?? '—'}</dd>
			<dt>Controller</dt>
			<dd class="mono selectable">{core.status?.controller ?? '—'}</dd>
			<dt>Proxy port</dt>
			<dd class="mono">{core.running ? (proxyPort ?? 'none') : (core.settings?.mixedPort ?? 'from profile')}</dd>
			<dt>Mode</dt>
			<dd>{core.settings?.mode ?? '—'}</dd>
			<dt>TUN</dt>
			<dd>{core.tunActive ? 'active' : 'off'}</dd>
		</dl>
	</Card>
</div>

<TunControl />

<Card title="Core log" padded={false}>
	<pre class="log selectable" bind:this={logEl}>{core.logs.join('\n') || 'No output yet.'}</pre>
</Card>

<style>
	.tiles {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
		gap: var(--space-3);
	}
	.grid {
		display: grid;
		grid-template-columns: minmax(0, 2fr) minmax(240px, 1fr);
		gap: var(--space-3);
	}
	.grid :global(.chart-card .body) {
		height: 240px;
	}
	.legend {
		display: flex;
		align-items: center;
		gap: var(--space-2);
		font-size: var(--text-xs);
		color: var(--color-text-muted);
	}
	.legend i {
		width: 10px;
		height: 3px;
		border-radius: 2px;
	}
	.dl {
		background: var(--color-download);
	}
	.ul {
		background: var(--color-upload);
		margin-left: var(--space-2);
	}
	dl {
		display: grid;
		grid-template-columns: auto 1fr;
		gap: var(--space-2) var(--space-4);
		margin: 0;
		align-items: center;
	}
	dt {
		color: var(--color-text-muted);
		font-size: var(--text-sm);
	}
	dd {
		margin: 0;
		text-align: right;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.log {
		height: 200px;
		margin: 0;
		padding: var(--space-3) var(--space-4);
		overflow: auto;
		font-family: var(--font-mono);
		font-size: var(--text-xs);
		white-space: pre-wrap;
		word-break: break-all;
	}
	.error {
		margin: 0;
		color: var(--color-danger);
		font-family: var(--font-mono);
		font-size: var(--text-xs);
		white-space: pre-wrap;
	}
</style>
