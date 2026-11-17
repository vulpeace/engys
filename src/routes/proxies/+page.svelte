<script lang="ts">
	import { ChevronDown, ChevronRight, Gauge, RefreshCw, Pin, PinOff, HeartPulse, Download, Undo2, Plus, Pencil, Trash2 } from '@lucide/svelte';
	import { Badge, Button, Card, EmptyState, Meter, PageHeader, Segmented, toast } from '#lib/ui/index.ts';
	import CustomGroupEditor, { type EditorActions } from '#lib/components/CustomGroupEditor.svelte';
	import type { CustomGroupsView } from '#lib/api/backend.ts';
	import { core } from '#lib/stores/core.svelte.ts';
	import type { Proxy, ProxyProvider } from '#lib/api/mihomo.ts';
	import { bytes, latencyTone, timeAgo } from '#lib/format.ts';

	const BUILTIN = new Set(['Direct', 'Reject', 'RejectDrop', 'Pass', 'PassRule', 'Compatible']);
	const SELECTABLE = new Set(['Selector', 'URLTest', 'Fallback']);

	let groupEditor = $state<EditorActions | null>(null);
	let groupContext = $state<CustomGroupsView | null>(null);
	let savingGroups = $state(false);
	let proxies = $state<Record<string, Proxy>>({});
	let providers = $state<ProxyProvider[]>([]);
	let loading = $state(false);
	/** Latest test results; overrides the history from /proxies. */
	let delays = $state<Record<string, number>>({});
	let testing = $state<Record<string, boolean>>({});
	let collapsed = $state<Record<string, boolean>>({});

	const testUrl = $derived(core.settings?.delayTestUrl ?? 'https://www.gstatic.com/generate_204');
	const timeout = $derived(core.settings?.delayTestTimeout ?? 5000);
	const mode = $derived(core.settings?.mode ?? 'rule');

	async function load() {
		const api = core.api;
		if (!api) return;
		loading = true;
		try {
			const [p, pr] = await Promise.all([api.proxies(), api.providers()]);
			proxies = p.proxies;
			// "default" (Compatible) is mihomo's internal provider for inline proxies.
			providers = Object.values(pr.providers).filter((x) => x.vehicleType !== 'Compatible');
		} catch (e) {
			toast.error(e);
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		if (core.api) load();
	});

	const isGroup = (p?: Proxy) => !!p?.all;

	// GLOBAL's member list preserves the config order of groups and proxies.
	const globalOrder = $derived(proxies.GLOBAL?.all ?? Object.keys(proxies));
	const groups = $derived.by(() => {
		const order = [...new Set([...globalOrder, ...Object.keys(proxies)])];
		const list = order.map((n) => proxies[n]).filter((p) => p && p.name !== 'GLOBAL' && isGroup(p) && !p.hidden);
		return mode === 'global' && proxies.GLOBAL ? [proxies.GLOBAL, ...list] : list;
	});
	const ungrouped = $derived.by(() => {
		const grouped = new Set<string>();
		for (const p of Object.values(proxies)) {
			if (isGroup(p) && p.name !== 'GLOBAL') p.all!.forEach((n) => grouped.add(n));
		}
		return globalOrder
			.map((n) => proxies[n])
			.filter((p) => p && !isGroup(p) && !BUILTIN.has(p.type) && !grouped.has(p.name));
	});

	function latency(name: string): number | undefined {
		if (name in delays) return delays[name];
		const p = proxies[name];
		if (!p) return undefined;
		if (isGroup(p) && p.now) return latency(p.now);
		const h = p.history;
		return h?.length ? h[h.length - 1].delay : undefined;
	}

	async function select(group: Proxy, name: string) {
		if (!SELECTABLE.has(group.type) || !core.api) return;
		try {
			await core.api.selectProxy(group.name, name);
			proxies[group.name] = { ...group, now: name, fixed: group.type === 'Selector' ? undefined : name };
		} catch (e) {
			toast.error(e);
		}
	}

	async function unpin(group: Proxy) {
		try {
			await core.api?.unfixProxy(group.name);
			await load();
		} catch (e) {
			toast.error(e);
		}
	}

	async function testGroup(group: Proxy) {
		if (!core.api) return;
		testing[group.name] = true;
		try {
			const res = await core.api.groupDelay(group.name, group.testUrl || testUrl, timeout);
			// Members missing from the result failed/timed out.
			for (const n of group.all ?? []) delays[n] = res[n] ?? 0;
			await load();
		} catch (e) {
			toast.error(e);
		} finally {
			testing[group.name] = false;
		}
	}

	async function testOne(name: string) {
		if (!core.api) return;
		testing[name] = true;
		try {
			delays[name] = (await core.api.proxyDelay(name, testUrl, timeout)).delay;
		} catch {
			delays[name] = 0;
		} finally {
			testing[name] = false;
		}
	}

	async function providerAction(name: string, action: 'update' | 'check') {
		if (!core.api) return;
		const key = `provider:${name}:${action}`;
		testing[key] = true;
		try {
			if (action === 'update') await core.api.updateProvider(name);
			else await core.api.healthcheckProvider(name);
			delays = {};
			await load();
			toast.success(action === 'update' ? `Updated ${name}` : `Health check done for ${name}`);
		} catch (e) {
			toast.error(e);
		} finally {
			testing[key] = false;
		}
	}

	async function changeMode(m: string) {
		try {
			await core.setMode(m as 'rule' | 'global' | 'direct');
		} catch (e) {
			toast.error(e);
		}
	}
</script>

{#snippet latencyBadge(name: string)}
	{@const ms = latency(name)}
	<span class="latency {latencyTone(ms)}">
		{#if testing[name]}…{:else if ms === undefined}—{:else if ms === 0}timeout{:else}{ms} ms{/if}
	</span>
{/snippet}

<PageHeader title="Proxies" subtitle="Switch proxy groups, test latency and refresh providers">
	{#snippet actions()}
		<Button disabled={!groupContext?.profileId || savingGroups} onclick={() => groupEditor?.edit()}><Plus size={14} /> Add group</Button>
		<Segmented
			label="Proxy mode"
			value={mode}
			disabled={!core.running}
			onValueChange={changeMode}
			options={[
				{ value: 'rule', label: 'Rule' },
				{ value: 'global', label: 'Global' },
				{ value: 'direct', label: 'Direct' }
			]}
		/>
		{#if core.settings?.overrides.includes('mode')}<Button icon title="Reset proxy mode to subscription" onclick={() => core.resetMode().catch(toast.error)}><Undo2 size={14} /></Button>{/if}
		<Button icon variant="ghost" title="Refresh" loading={loading} disabled={!core.running} onclick={load}>
			<RefreshCw size={16} />
		</Button>
	{/snippet}
</PageHeader>

<CustomGroupEditor onReady={(actions) => groupEditor = actions} onChange={(view) => groupContext = view} onBusy={(busy) => savingGroups = busy} />

{#if !core.running}
	<Card><EmptyState title="Core is stopped" description="Start the core on the Dashboard to select proxies and test latency. Custom groups can be created while stopped." /></Card>
{:else}
	{#if mode === 'direct'}
		<Card><p class="muted">Direct mode: all traffic bypasses proxies. Group selections still apply when you switch back.</p></Card>
	{/if}

	{#each groups as group (group.name)}
		{@const open = !collapsed[group.name]}
		{@const custom = groupContext?.groups.find((g) => g.name === group.name)}
		<Card padded={false}>
			<div class="group-head">
				<button class="toggle" onclick={() => (collapsed[group.name] = open)} aria-expanded={open}>
					{#if open}<ChevronDown size={16} />{:else}<ChevronRight size={16} />{/if}
					<span class="group-name">{group.name}</span>
					<Badge>{group.type}</Badge>
					{#if custom}<Badge tone="accent">Custom</Badge>{/if}
					{#if group.now}<span class="muted now">→ {group.now}</span>{/if}
					{@render latencyBadge(group.name)}
				</button>
				<div class="group-actions">
					{#if custom}
						<Button icon size="sm" disabled={savingGroups} title={`Edit ${group.name}`} onclick={() => groupEditor?.edit(custom)}><Pencil size={14} /></Button>
						<Button icon size="sm" variant="danger" disabled={savingGroups} title={`Delete ${group.name}`} onclick={() => groupEditor?.remove(custom)}><Trash2 size={14} /></Button>
					{/if}
					{#if group.fixed}
						<Button size="sm" variant="ghost" title="Unpin: return to automatic selection" onclick={() => unpin(group)}>
							<PinOff size={14} /> Unpin
						</Button>
					{/if}
					<Button size="sm" loading={testing[group.name]} onclick={() => testGroup(group)}>
						<Gauge size={14} /> Test
					</Button>
				</div>
			</div>
			{#if open}
				<div class="members">
					{#each group.all ?? [] as name (name)}
						{@const member = proxies[name]}
						<button
							class="member"
							class:selected={group.now === name}
							disabled={savingGroups || !SELECTABLE.has(group.type)}
							onclick={() => select(group, name)}
						>
							<span class="member-name">
								{#if group.fixed === name}<Pin size={12} />{/if}
								{name}
							</span>
							<span class="member-meta">
								<span class="muted">{member?.type ?? ''}</span>
								{@render latencyBadge(name)}
							</span>
						</button>
					{/each}
				</div>
			{/if}
		</Card>
	{:else}
		{#if ungrouped.length === 0}
			<Card><EmptyState title="No proxy groups" description="The active profile doesn't define any proxy groups or proxies." /></Card>
		{/if}
	{/each}

	{#if ungrouped.length}
		<Card title="Ungrouped proxies">
			{#snippet actions()}
				{#if mode !== 'global'}
					<Button size="sm" onclick={() => changeMode('global')}>Switch to Global</Button>
				{/if}
			{/snippet}
			<p class="muted hint">
				These proxies aren't part of any group. Selecting one routes traffic through it in Global mode.
			</p>
			<div class="members flush">
				{#each ungrouped as p (p.name)}
					<div class="member" class:selected={proxies.GLOBAL?.now === p.name}>
						<button class="member-name linklike" onclick={() => proxies.GLOBAL && select(proxies.GLOBAL, p.name)}>
							{p.name}
						</button>
						<span class="member-meta">
							<span class="muted">{p.type}</span>
							<button class="linklike" onclick={() => testOne(p.name)} title="Test latency">
								{@render latencyBadge(p.name)}
							</button>
						</span>
					</div>
				{/each}
			</div>
		</Card>
	{/if}

	{#if providers.length}
		<Card title="Proxy providers" padded={false}>
			<ul class="providers">
				{#each providers as pr (pr.name)}
					{@const sub = pr.subscriptionInfo}
					<li>
						<div class="provider-info">
							<div class="provider-title">
								<strong>{pr.name}</strong>
								<Badge>{pr.vehicleType}</Badge>
								<span class="muted">{pr.proxies.length} proxies</span>
							</div>
							<span class="muted small">
								{pr.updatedAt ? `Updated ${timeAgo(Date.parse(pr.updatedAt) / 1000)}` : 'Never updated'}
								{#if sub?.Total}· {bytes(sub.Upload + sub.Download)} / {bytes(sub.Total)}{/if}
							</span>
							{#if sub?.Total}<Meter value={sub.Upload + sub.Download} max={sub.Total} label="Traffic used" />{/if}
						</div>
						<div class="group-actions">
							<Button size="sm" loading={testing[`provider:${pr.name}:check`]} onclick={() => providerAction(pr.name, 'check')}>
								<HeartPulse size={14} /> Health check
							</Button>
							{#if pr.vehicleType === 'HTTP'}
								<Button size="sm" loading={testing[`provider:${pr.name}:update`]} onclick={() => providerAction(pr.name, 'update')}>
									<Download size={14} /> Fetch
								</Button>
							{/if}
						</div>
					</li>
				{/each}
			</ul>
		</Card>
	{/if}
{/if}

<style>
	.group-head {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: var(--space-3);
		padding: var(--space-3) var(--space-4);
	}
	.toggle {
		display: flex;
		align-items: center;
		gap: var(--space-2);
		min-width: 0;
		padding: 0;
		border: none;
		background: none;
		cursor: pointer;
		text-align: left;
	}
	.group-name {
		font-weight: var(--weight-bold);
	}
	.now {
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.group-actions {
		display: flex;
		gap: var(--space-2);
		flex-shrink: 0;
	}
	.members {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
		gap: var(--space-2);
		padding: 0 var(--space-4) var(--space-4);
	}
	.members.flush {
		padding: 0;
	}
	.member {
		display: flex;
		flex-direction: column;
		gap: var(--space-1);
		padding: var(--space-2) var(--space-3);
		background: var(--color-surface-2);
		border: 1px solid transparent;
		border-radius: var(--radius-md);
		text-align: left;
		cursor: pointer;
		min-width: 0;
	}
	.member:disabled {
		cursor: default;
	}
	.member:hover:not(:disabled) {
		border-color: var(--color-border);
	}
	.member.selected {
		border-color: var(--color-accent);
		background: var(--color-accent-soft);
	}
	.member-name {
		display: flex;
		align-items: center;
		gap: var(--space-1);
		font-weight: var(--weight-medium);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.member-meta {
		display: flex;
		justify-content: space-between;
		font-size: var(--text-xs);
	}
	.linklike {
		padding: 0;
		border: none;
		background: none;
		cursor: pointer;
		text-align: left;
	}
	.latency {
		font-size: var(--text-xs);
		font-variant-numeric: tabular-nums;
		color: var(--color-text-muted);
	}
	.latency.good {
		color: var(--color-success);
	}
	.latency.ok {
		color: var(--color-warning);
	}
	.latency.bad,
	.latency.fail {
		color: var(--color-danger);
	}
	.hint {
		margin-bottom: var(--space-3);
		font-size: var(--text-sm);
	}
	.providers {
		margin: 0;
		padding: 0;
		list-style: none;
	}
	.providers li {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: var(--space-4);
		padding: var(--space-3) var(--space-4);
	}
	.providers li + li {
		border-top: 1px solid var(--color-border);
	}
	.provider-info {
		display: flex;
		flex-direction: column;
		gap: var(--space-1);
		flex: 1;
		min-width: 0;
	}
	.provider-title {
		display: flex;
		align-items: center;
		gap: var(--space-2);
	}
	.small {
		font-size: var(--text-xs);
	}
</style>
