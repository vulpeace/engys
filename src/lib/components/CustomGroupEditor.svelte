<script module lang="ts">
	import type { CustomProxyGroup } from '#lib/api/backend.ts';
	export type EditorActions = { edit: (group?: CustomProxyGroup) => void; remove: (group: CustomProxyGroup) => Promise<void> };
</script>

<script lang="ts">
	import { onMount } from 'svelte';
	import { listen } from '@tauri-apps/api/event';
	import { ask } from '@tauri-apps/plugin-dialog';
	import { ArrowUp, ArrowDown, Pencil, Trash2, Plus } from '@lucide/svelte';
	import { Badge, Button, Card, Dialog, Field, Select, Switch, TextField, toast } from '#lib/ui/index.ts';
	import { backend, type CustomGroupsView } from '#lib/api/backend.ts';
	import { core } from '#lib/stores/core.svelte.ts';

	let { onReady, onChange, onBusy }: {
		onReady: (actions: EditorActions) => void;
		onChange: (view: CustomGroupsView) => void;
		onBusy: (busy: boolean) => void;
	} = $props();
	let context = $state<CustomGroupsView | null>(null);
	let open = $state(false);
	let saving = $state(false);
	let loadError = $state('');
	let formError = $state('');
	let profileId = $state('');
	let profileName = $state('');
	let editingId = $state('');
	let name = $state('');
	let kind = $state<CustomProxyGroup['kind']>('select');
	let members = $state<string[]>([]);
	let providers = $state<string[]>([]);
	let includeAllProxies = $state(false);
	let includeAllProviders = $state(false);
	let filter = $state('');
	let url = $state('');
	let interval = $state('300');
	let strategy = $state<CustomProxyGroup['strategy']>('consistent-hashing');
	let member = $state('');
	const memberOptions = $derived((context?.members ?? [])
		.filter((value) => value !== name && !members.includes(value)).map((value) => ({ value, label: value })));
	const validInterval = $derived(Number.isInteger(Number(interval)) && Number(interval) > 0 && Number(interval) <= 4294967295);

	function accept(view: CustomGroupsView) { context = view; onChange(view); loadError = ''; }
	async function load() {
		try { accept(await backend.customGroupsGet()); }
		catch (e) { loadError = String(e); }
	}
	onMount(() => {
		onReady({ edit, remove });
		void load();
		const listeners = ['profiles-changed', 'custom-groups-changed'].map((event) => listen(event, () => { void load(); }).catch(toast.error));
		return () => { for (const listener of listeners) void listener.then((stop) => stop?.()); };
	});

	function edit(group?: CustomProxyGroup) {
		if (!context?.profileId) return;
		profileId = context.profileId;
		profileName = context.profileName ?? 'this profile';
		editingId = group?.id ?? '';
		name = group?.name ?? '';
		kind = group?.kind ?? 'select';
		members = [...(group?.proxies ?? [])];
		providers = [...(group?.providers ?? [])];
		includeAllProxies = group?.includeAllProxies ?? false;
		includeAllProviders = group?.includeAllProviders ?? false;
		filter = group?.filter ?? '';
		url = group?.url ?? core.settings?.delayTestUrl ?? 'https://www.gstatic.com/generate_204';
		interval = String(group?.interval ?? 300);
		strategy = group?.strategy ?? 'consistent-hashing';
		member = '';
		formError = '';
		open = true;
	}

	async function commit(groups: CustomProxyGroup[], id: string) {
		saving = true;
		onBusy(true);
		try {
			accept(await backend.customGroupsSave(id, groups));
			toast.success(core.running ? 'Custom groups saved and applied' : 'Custom groups saved for the next core start');
			return true;
		} catch (e) { formError = String(e); toast.error(e); return false; }
		finally { saving = false; onBusy(false); }
	}

	async function save() {
		if (!context) return;
		formError = '';
		const group: CustomProxyGroup = {
			id: editingId || crypto.randomUUID(), name: name.trim(), kind, proxies: $state.snapshot(members),
			providers: $state.snapshot(providers), includeAllProxies, includeAllProviders, filter,
			url: url.trim(), interval: kind === 'select' ? 300 : Number(interval), strategy
		};
		const groups = $state.snapshot(context.groups).filter((g) => g.id !== editingId);
		const index = editingId ? context.groups.findIndex((g) => g.id === editingId) : groups.length;
		groups.splice(index < 0 ? groups.length : index, 0, group);
		if (await commit(groups, profileId)) open = false;
	}

	async function remove(group: CustomProxyGroup) {
		if (!context?.profileId || saving) return;
		if (!(await ask(`Delete custom group "${group.name}"? Rules or groups using it must be changed first.`, { title: 'Delete custom group', kind: 'warning' }))) return;
		await commit($state.snapshot(context.groups).filter((g) => g.id !== group.id), context.profileId);
	}
	function move(index: number, delta: number) {
		const reordered = [...members];
		[reordered[index], reordered[index + delta]] = [reordered[index + delta], reordered[index]];
		members = reordered;
	}
	function toggleProvider(value: string, enabled: boolean) {
		providers = enabled ? [...providers, value] : providers.filter((p) => p !== value);
	}
</script>

{#if loadError}<Card><p class="error" role="alert">Could not load custom groups: {loadError}</p><Button onclick={load}>Retry</Button></Card>{/if}
{#if context && !context.profileId}<p class="muted hint">Choose an active profile on Profiles before adding custom groups.</p>{/if}
{#if context && !core.running && context.groups.length}
	<Card title="Custom groups">
		<p class="muted hint">Saved for {context.profileName}. Start the core to select proxies and test these groups.</p>
		<ul class="saved-groups">
			{#each context.groups as group (group.id)}
				<li><span>{group.name}</span><Badge>{group.kind}</Badge><div class="row-actions">
					<Button icon size="sm" disabled={saving} title={`Edit ${group.name}`} onclick={() => edit(group)}><Pencil size={14} /></Button>
					<Button icon size="sm" variant="danger" disabled={saving} title={`Delete ${group.name}`} onclick={() => remove(group)}><Trash2 size={14} /></Button>
				</div></li>
			{/each}
		</ul>
	</Card>
{/if}

<Dialog bind:open title={editingId ? 'Edit custom proxy group' : 'Add custom proxy group'} description={`Saved for ${profileName}. Use it as a routing target in Proxied Apps or through a parent group.`}>
	<Field label="Name" stacked id="group-name"><TextField id="group-name" bind:value={name} disabled={saving} placeholder="My proxies" /></Field>
	<Field label="Type" stacked><Select bind:value={kind} disabled={saving} label="Group type" options={[
		{ value: 'select', label: 'Select — choose manually' }, { value: 'url-test', label: 'URL-test — fastest available' },
		{ value: 'fallback', label: 'Fallback — first available' }, { value: 'load-balance', label: 'Load-balance' }
	]} /></Field>
	<Field label="All profile proxies"><Switch label="Include all profile proxies" bind:checked={includeAllProxies} disabled={saving} /></Field>
	<Field label="All subscription providers"><Switch label="Include all subscription providers" bind:checked={includeAllProviders} disabled={saving} /></Field>
	<Field label="Add a member" stacked description="Select proxies or other groups. Member order sets preference for Select and Fallback.">
		<Select bind:value={member} options={memberOptions} label="Member to add" disabled={saving} placeholder="Choose a proxy or group" />
		<Button icon disabled={saving || !member} title="Add selected member" onclick={() => { members.push(member); member = ''; }}><Plus size={14} /></Button>
	</Field>
	{#if members.length}<ol class="selected-members">
		{#each members as item, i (item)}<li><span class:missing={!context?.members.includes(item)}>{item}{!context?.members.includes(item) ? ' (unavailable)' : ''}</span><div class="row-actions">
			<Button icon size="sm" disabled={saving || i === 0} title="Move member up" onclick={() => move(i, -1)}><ArrowUp size={14} /></Button>
			<Button icon size="sm" disabled={saving || i === members.length - 1} title="Move member down" onclick={() => move(i, 1)}><ArrowDown size={14} /></Button>
			<Button icon size="sm" disabled={saving} title="Remove member" onclick={() => members.splice(i, 1)}><Trash2 size={14} /></Button>
		</div></li>{/each}
	</ol>{/if}
	{#if !includeAllProviders}
		<div class="provider-choices"><span class="label">Subscription providers</span>
			{#each [...new Set([...(context?.providers ?? []), ...providers])] as provider (provider)}
				<label class="choice"><input type="checkbox" checked={providers.includes(provider)} disabled={saving} onchange={(e) => toggleProvider(provider, e.currentTarget.checked)} /><span>{provider}{!context?.providers.includes(provider) ? ' (unavailable)' : ''}</span></label>
			{:else}<p class="muted hint">This profile has no proxy providers.</p>{/each}
		</div>
	{/if}
	<Field label="Member name filter" stacked id="group-filter" description="Optional regular expression; mihomo validates the syntax."><TextField id="group-filter" bind:value={filter} disabled={saving} placeholder="e.g. (?i)japan|singapore" /></Field>
	<Field label="Test URL" stacked id="group-url"><TextField id="group-url" type="url" bind:value={url} disabled={saving} /></Field>
	{#if kind !== 'select'}<Field label="Test interval (seconds)" stacked id="group-interval"><TextField id="group-interval" type="number" min="1" max="4294967295" step="1" bind:value={interval} disabled={saving} /></Field>{/if}
	{#if kind === 'load-balance'}<Field label="Strategy" stacked><Select bind:value={strategy} disabled={saving} label="Load-balance strategy" options={[
		{ value: 'consistent-hashing', label: 'Consistent hashing' }, { value: 'round-robin', label: 'Round robin' }, { value: 'sticky-sessions', label: 'Sticky sessions' }
	]} /></Field>{/if}
	{#if formError}<p class="error selectable" role="alert">{formError}</p>{/if}
	{#snippet footer()}<Button disabled={saving} onclick={() => open = false}>Cancel</Button><Button variant="primary" loading={saving} disabled={!name.trim() || !url.trim() || (kind !== 'select' && !validInterval) || (!members.length && !providers.length && !includeAllProxies && !includeAllProviders)} onclick={save}>Save & apply</Button>{/snippet}
</Dialog>

<style>
	.hint { font-size: var(--text-xs); }
	.error, .missing { color: var(--color-danger); font-size: var(--text-sm); white-space: pre-wrap; overflow-wrap: anywhere; }
	.saved-groups, .selected-members { list-style: none; padding: 0; margin: 0; }
	.saved-groups li, .selected-members li { display: flex; align-items: center; gap: var(--space-2); padding: var(--space-2) 0; }
	.saved-groups li + li, .selected-members li + li { border-top: 1px solid var(--color-border); }
	.selected-members li > span, .saved-groups li > span { flex: 1; min-width: 0; overflow-wrap: anywhere; }
	.row-actions { display: flex; gap: var(--space-1); margin-left: auto; }
	.provider-choices { display: flex; flex-direction: column; gap: var(--space-2); }
	.label { font-weight: var(--weight-medium); }
	.choice { display: flex; gap: var(--space-2); align-items: center; font-size: var(--text-sm); }
	.choice input { accent-color: var(--color-accent); }
</style>
