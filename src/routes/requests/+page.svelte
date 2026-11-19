<script lang="ts">
	import { Pause, Play, Trash2, X, XCircle } from '@lucide/svelte';
	import { Badge, Button, Card, Dialog, EmptyState, PageHeader, Segmented, TextField, toast } from '#lib/ui/index.ts';
	import VirtualList from '#lib/components/VirtualList.svelte';
	import { core } from '#lib/stores/core.svelte.ts';
	import { live, type ConnRecord } from '#lib/stores/live.svelte.ts';
	import { bytes, clockTime, speed } from '#lib/format.ts';

	let query = $state('');
	let status = $state<'all' | 'active' | 'closed'>('all');
	let paused = $state(false);
	let frozen = $state.raw<ConnRecord[]>([]);
	let detail = $state<ConnRecord | null>(null);
	let detailOpen = $state(false);

	const source = $derived(paused ? frozen : live.records);

	const target = (r: ConnRecord) => {
		const m = r.conn.metadata;
		return `${m.host || m.sniffHost || m.destinationIP}:${m.destinationPort}`;
	};
	// mihomo lists chains from outermost to innermost; show them in route order.
	const chain = (r: ConnRecord) => [...r.conn.chains].reverse().join(' → ');
	const rule = (r: ConnRecord) => (r.conn.rulePayload ? `${r.conn.rule}(${r.conn.rulePayload})` : r.conn.rule);

	const rows = $derived.by(() => {
		const q = query.trim().toLowerCase();
		return source.filter((r) => {
			if (status === 'active' && r.closedAt) return false;
			if (status === 'closed' && !r.closedAt) return false;
			if (!q) return true;
			const m = r.conn.metadata;
			return [target(r), m.process, m.processPath, chain(r), rule(r), m.sourceIP, m.network, m.type]
				.filter(Boolean)
				.some((s) => s!.toLowerCase().includes(q));
		});
	});

	function togglePause() {
		paused = !paused;
		if (paused) frozen = live.records;
	}

	async function closeOne(r: ConnRecord) {
		try {
			await core.api?.closeConnection(r.conn.id);
		} catch (e) {
			toast.error(e);
		}
	}

	async function closeAll() {
		try {
			await core.api?.closeAllConnections();
		} catch (e) {
			toast.error(e);
		}
	}

	function openDetail(r: ConnRecord) {
		detail = r;
		detailOpen = true;
	}
</script>

<PageHeader title="Requests" subtitle="{live.activeCount} active · {live.records.length} in history (limit {live.historyLimit})">
	{#snippet actions()}
		<Button onclick={togglePause} title={paused ? 'Resume live updates' : 'Freeze the list'}>
			{#if paused}<Play size={14} /> Resume{:else}<Pause size={14} /> Pause{/if}
		</Button>
		<Button onclick={() => live.clearHistory()} title="Remove closed connections from the list">
			<Trash2 size={14} /> Clear closed
		</Button>
		<Button variant="danger" disabled={!core.running || live.activeCount === 0} onclick={closeAll}>
			<XCircle size={14} /> Close all
		</Button>
	{/snippet}
</PageHeader>

<div class="toolbar">
	<TextField placeholder="Filter by host, process, rule, chain…" bind:value={query} />
	<Segmented
		label="Connection status"
		bind:value={status}
		options={[
			{ value: 'all', label: 'All' },
			{ value: 'active', label: 'Active' },
			{ value: 'closed', label: 'Closed' }
		]}
	/>
</div>

<Card padded={false} class="table-card">
	<div class="body">
		<VirtualList
			items={rows}
			rowHeight={34}
			key={(r) => r.conn.id}
			minWidth="calc(766px + 8 * var(--space-3) + 2 * var(--space-4))"
		>
			{#snippet header()}
				<div class="row head">
					<span>Time</span><span>Destination</span><span>Process</span><span>Rule</span><span>Chain</span>
					<span class="r">↓</span><span class="r">↑</span><span class="r">Speed</span><span></span>
				</div>
			{/snippet}
			{#snippet empty()}
				<EmptyState
					title={source.length ? 'No matches' : 'No requests yet'}
					description={core.running ? undefined : 'Start the core to see traffic.'}
				/>
			{/snippet}
			{#snippet row(r: ConnRecord)}
				{@const m = r.conn.metadata}
				<div class="row" class:closed={r.closedAt} role="button" tabindex="0" onclick={() => openDetail(r)} onkeydown={(e) => e.key === 'Enter' && openDetail(r)}>
					<span class="mono muted">{clockTime(r.conn.start)}</span>
					<span class="ellipsis" title={target(r)}>
						<Badge>{m.network}</Badge>
						{target(r)}
					</span>
					<span class="ellipsis" title={m.processPath}>{m.process || '—'}</span>
					<span class="ellipsis" title={rule(r)}>{rule(r)}</span>
					<span class="ellipsis" title={chain(r)}>{chain(r)}</span>
					<span class="r mono">{bytes(r.conn.download)}</span>
					<span class="r mono">{bytes(r.conn.upload)}</span>
					<span class="r mono muted">{r.closedAt ? '—' : speed(r.downSpeed + r.upSpeed)}</span>
					<span class="r">
						{#if !r.closedAt}
							<button class="close" title="Close connection" onclick={(e) => { e.stopPropagation(); closeOne(r); }}>
								<X size={14} />
							</button>
						{/if}
					</span>
				</div>
			{/snippet}
		</VirtualList>
	</div>
</Card>

<Dialog bind:open={detailOpen} title="Connection details" description={detail ? target(detail) : ''}>
	{#if detail}
		<dl>
			<dt>Status</dt><dd>{detail.closedAt ? `Closed at ${clockTime(detail.closedAt)}` : 'Active'}</dd>
			<dt>Started</dt><dd>{new Date(detail.conn.start).toLocaleString()}</dd>
			<dt>Rule</dt><dd>{rule(detail)}</dd>
			<dt>Chain</dt><dd>{chain(detail)}</dd>
			<dt>Traffic</dt><dd>↓ {bytes(detail.conn.download)} · ↑ {bytes(detail.conn.upload)}</dd>
		</dl>
		<pre class="json selectable">{JSON.stringify(detail.conn.metadata, null, 2)}</pre>
	{/if}
</Dialog>

<style>
	.toolbar {
		display: flex;
		gap: var(--space-3);
		align-items: center;
	}
	:global(.table-card) {
		flex: 1;
		min-height: 320px;
		overflow: hidden;
	}
	:global(.table-card > .body) {
		display: flex;
		flex-direction: column;
	}
	.body {
		flex: 1;
		min-height: 0;
		position: relative;
	}
	.body > :global(*) {
		position: absolute;
		inset: 0;
	}
	.row {
		display: grid;
		grid-template-columns: 70px minmax(160px, 2fr) minmax(80px, 1fr) minmax(100px, 1.2fr) minmax(100px, 1.2fr) 72px 72px 80px 32px;
		gap: var(--space-3);
		align-items: center;
		height: 34px;
		padding: 0 var(--space-4);
		font-size: var(--text-sm);
		border-bottom: 1px solid var(--color-border);
		cursor: pointer;
	}
	.row:hover:not(.head) {
		background: var(--color-surface-2);
	}
	.head {
		cursor: default;
		font-size: var(--text-xs);
		font-weight: var(--weight-bold);
		color: var(--color-text-muted);
		text-transform: uppercase;
		letter-spacing: 0.03em;
	}
	.closed {
		opacity: 0.55;
	}
	.ellipsis {
		display: flex;
		align-items: center;
		gap: var(--space-2);
		min-width: 0;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.r {
		text-align: right;
		justify-self: end;
	}
	.mono {
		font-size: var(--text-xs);
	}
	.close {
		display: inline-flex;
		padding: 2px;
		border: none;
		border-radius: var(--radius-sm);
		background: none;
		color: var(--color-text-muted);
		cursor: pointer;
	}
	.close:hover {
		color: var(--color-danger);
		background: var(--color-danger-soft);
	}
	dl {
		display: grid;
		grid-template-columns: auto 1fr;
		gap: var(--space-1) var(--space-4);
		margin: 0;
		font-size: var(--text-sm);
	}
	dt {
		color: var(--color-text-muted);
	}
	dd {
		margin: 0;
		word-break: break-all;
	}
	.json {
		margin: 0;
		padding: var(--space-3);
		background: var(--color-surface-2);
		border-radius: var(--radius-md);
		font-family: var(--font-mono);
		font-size: var(--text-xs);
		overflow: auto;
	}
</style>
