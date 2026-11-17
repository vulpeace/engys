<script lang="ts">
	import { onMount } from 'svelte';
	import { listen } from '@tauri-apps/api/event';
	import { FilePlus, FolderOpen, Link, Pencil, RefreshCw, Trash2, Check, Undo2 } from '@lucide/svelte';
	import { ask, open as openFile } from '@tauri-apps/plugin-dialog';
	import { Badge, Button, Card, Dialog, EmptyState, Meter, PageHeader, TextField, toast } from '#lib/ui/index.ts';
	import YamlEditor from '#lib/components/YamlEditor.svelte';
	import { backend, type Profile, type ProfileIndex } from '#lib/api/backend.ts';
	import { core } from '#lib/stores/core.svelte.ts';
	import { bytes, dateTime, timeAgo } from '#lib/format.ts';

	const TEMPLATE = `# Minimal mihomo profile — see Meta-Docs/docs/config for every option.
proxies:
  - name: example-ss
    type: ss
    server: example.com
    port: 8388
    cipher: aes-128-gcm
    password: change-me

proxy-groups:
  - name: Proxy
    type: select
    proxies: [example-ss, DIRECT]

rules:
  - MATCH,Proxy
`;

	let index = $state<ProfileIndex>({ active: null, profiles: [] });
	let busy = $state<Record<string, boolean>>({});

	// Dialog state
	let subOpen = $state(false);
	let subUrl = $state('');
	let subName = $state('');
	let subInterval = $state('0');
	let subIntervalTouched = $state(false);
	let intervalDrafts = $state<Record<string, string>>({});
	const validInterval = (input: string | number) => String(input ?? '').trim() !== '' && Number.isInteger(Number(input)) && Number(input) >= 0 && Number(input) <= 4294967295;
	let editorOpen = $state(false);
	let editing = $state<{ id: string | null; name: string; content: string }>({ id: null, name: '', content: '' });

	async function refresh() {
		index = await backend.profilesList();
		await core.refreshSettings();
	}
	refresh().catch(toast.error);
	onMount(() => {
		const unlisten = listen<ProfileIndex>('profiles-changed', (event) => { index = event.payload; }).catch(toast.error);
		return () => { void unlisten.then((stop) => stop?.()); };
	});

	async function run(key: string, fn: () => Promise<unknown>, ok?: string) {
		busy[key] = true;
		try {
			await fn();
			await refresh();
			if (ok) toast.success(ok);
			return true;
		} catch (e) {
			toast.error(e);
			return false;
		} finally {
			busy[key] = false;
		}
	}

	async function importFile() {
		const path = await openFile({
			multiple: false,
			directory: false,
			filters: [{ name: 'YAML profile', extensions: ['yaml', 'yml'] }, { name: 'All files', extensions: ['*'] }]
		});
		if (typeof path === 'string') await run('import', () => backend.profileImportFile(path), 'Profile imported');
	}

	async function addSubscription() {
		const ok = await run('sub', () => backend.profileAddRemote(subUrl, subName || undefined, Number(subInterval), subIntervalTouched), 'Subscription added');
		if (ok) {
			subOpen = false;
			subUrl = subName = '';
			subInterval = '0';
			subIntervalTouched = false;
		}
	}

	function newInline() {
		editing = { id: null, name: 'New profile', content: TEMPLATE };
		editorOpen = true;
	}

	async function edit(p: Profile) {
		try {
			editing = { id: p.id, name: p.name, content: await backend.profileRead(p.id) };
			editorOpen = true;
		} catch (e) {
			toast.error(e);
		}
	}

	async function saveEditor() {
		const { id, name, content } = editing;
		const ok = await run('save', async () => {
			if (id) {
				await backend.profileSave(id, content);
				const current = index.profiles.find((p) => p.id === id);
				if (current && current.name !== name) await backend.profileRename(id, name);
			} else {
				await backend.profileCreateInline(name, content);
			}
		}, id ? 'Profile saved' : 'Profile created');
		if (ok) editorOpen = false;
	}

	const activate = (p: Profile) =>
		run(`activate:${p.id}`, () => backend.profileActivate(p.id), core.running ? `Switched to ${p.name}` : undefined);

	const update = (p: Profile) => run(`update:${p.id}`, () => backend.profileUpdateRemote(p.id), `Updated ${p.name}`);

	async function saveInterval(p: Profile) {
		const value = intervalDrafts[p.id] ?? String(p.updateIntervalMinutes);
		if (!validInterval(value)) { toast.error('Enter a whole number of minutes, 0 or greater.'); return; }
		const ok = await run(`interval:${p.id}`, () => backend.profileSetUpdateInterval(p.id, Number(value)), 'Update interval saved');
		if (ok) delete intervalDrafts[p.id];
	}

	async function resetInterval(p: Profile) {
		const ok = await run(`interval:${p.id}`, () => backend.profileResetUpdateInterval(p.id), 'Subscription controls the update interval');
		if (ok) delete intervalDrafts[p.id];
	}

	async function remove(p: Profile) {
		if (!(await ask(`Delete profile "${p.name}"?`, { title: 'Delete profile', kind: 'warning' }))) return;
		await run(`delete:${p.id}`, () => backend.profileDelete(p.id));
	}

	const kindLabel = { inline: 'Inline', file: 'File', remote: 'Subscription' } as const;
</script>

<PageHeader title="Profiles" subtitle="Configs the core can run. The active one is used on start.">
	{#snippet actions()}
		<Button onclick={newInline}><FilePlus size={14} /> New inline</Button>
		<Button loading={busy.import} onclick={importFile}><FolderOpen size={14} /> Import file</Button>
		<Button variant="primary" onclick={() => (subOpen = true)}><Link size={14} /> Add subscription</Button>
	{/snippet}
</PageHeader>

{#if index.profiles.length === 0}
	<Card>
		<EmptyState
			title="No profiles yet"
			description="Create one inline, import a YAML file, or add a subscription link using the buttons above."
		/>
	</Card>
{:else}
	<div class="list">
		{#each index.profiles as p (p.id)}
			{@const active = index.active === p.id}
			{@const sub = p.subscription}
			<Card>
				<div class="profile" class:active>
					<div class="info">
						<div class="title">
							<strong>{p.name}</strong>
							<Badge tone={p.kind === 'remote' ? 'accent' : 'neutral'}>{kindLabel[p.kind]}</Badge>
							{#if active}<Badge tone="success"><Check size={12} /> Active</Badge>{/if}
						</div>
						{#if p.source}<div class="source muted mono selectable" title={p.source}>{p.source}</div>{/if}
						<div class="muted small">Updated {timeAgo(p.updatedAt)}</div>
						{#if p.kind === 'remote'}
							<div class="interval-control">
								<label class="small" for={`interval-${p.id}`}>Update interval (minutes)</label>
								<div class="interval-input">
									<TextField id={`interval-${p.id}`} type="number" min="0" max="4294967295" step="1"
										value={intervalDrafts[p.id] ?? String(p.updateIntervalMinutes)} disabled={busy[`interval:${p.id}`]}
										oninput={(e) => intervalDrafts[p.id] = e.currentTarget.value} />
									<Button size="sm" loading={busy[`interval:${p.id}`]}
										disabled={!validInterval(intervalDrafts[p.id] ?? String(p.updateIntervalMinutes)) || Number(intervalDrafts[p.id] ?? p.updateIntervalMinutes) === p.updateIntervalMinutes}
										onclick={() => saveInterval(p)}>Save</Button>
									{#if p.updateIntervalSource === 'client' || Number(intervalDrafts[p.id] ?? p.updateIntervalMinutes) !== p.updateIntervalMinutes}
										<Button icon size="sm" disabled={busy[`interval:${p.id}`]} title="Reset to subscription interval" onclick={() => resetInterval(p)}><Undo2 size={14} /></Button>
									{/if}
								</div>
								<span class="muted small">0 disables automatic refresh.{p.updateIntervalSource === 'subscription' ? ' Following the subscription interval.' : ''}{p.lastRefreshedAt ? ` Last fetched ${timeAgo(p.lastRefreshedAt)}.` : ''}</span>
								{#if p.lastRefreshError}<p class="refresh-error small selectable" role="status">Refresh failed: {p.lastRefreshError}</p>{/if}
							</div>
						{/if}
						{#if sub && sub.total > 0}
							<div class="usage">
								<Meter value={sub.upload + sub.download} max={sub.total} label="Subscription traffic" />
								<span class="muted small">
									{bytes(sub.upload + sub.download)} of {bytes(sub.total)}
									{#if sub.expire}· expires {dateTime(sub.expire)}{/if}
								</span>
							</div>
						{/if}
					</div>
					<div class="actions">
						{#if !active}
							<Button size="sm" variant="primary" loading={busy[`activate:${p.id}`]} onclick={() => activate(p)}>Use</Button>
						{/if}
						{#if p.kind === 'remote'}
							<Button size="sm" icon title="Update subscription" loading={busy[`update:${p.id}`]} onclick={() => update(p)}>
								<RefreshCw size={14} />
							</Button>
						{/if}
						<Button size="sm" icon title="Edit" onclick={() => edit(p)}><Pencil size={14} /></Button>
						<Button size="sm" icon variant="danger" title="Delete" loading={busy[`delete:${p.id}`]} onclick={() => remove(p)}>
							<Trash2 size={14} />
						</Button>
					</div>
				</div>
			</Card>
		{/each}
	</div>
{/if}

<Dialog bind:open={subOpen} title="Add subscription" description="The link must return a Clash/mihomo YAML config.">
	<label class="label" for="sub-url">Subscription URL</label>
	<TextField id="sub-url" placeholder="https://…" bind:value={subUrl} />
	<label class="label" for="sub-name">Name (optional)</label>
	<TextField id="sub-name" placeholder="Taken from the server if empty" bind:value={subName} />
	<label class="label" for="sub-interval">Update interval (minutes)</label>
	<TextField id="sub-interval" type="number" min="0" max="4294967295" step="1" bind:value={subInterval} oninput={() => subIntervalTouched = true} />
	<p class="muted small">0 disables automatic refresh. Refresh runs while the app is open, including in the tray.</p>
	{#snippet footer()}
		<Button onclick={() => (subOpen = false)}>Cancel</Button>
		<Button variant="primary" loading={busy.sub} disabled={!/^https?:\/\//.test(subUrl.trim()) || !validInterval(String(subInterval))} onclick={addSubscription}>
			Download
		</Button>
	{/snippet}
</Dialog>

<Dialog bind:open={editorOpen} wide title={editing.id ? 'Edit profile' : 'New inline profile'}>
	<TextField placeholder="Profile name" bind:value={editing.name} />
	{#if editing.id && index.profiles.find((p) => p.id === editing.id)?.kind === 'remote'}
		<p class="muted small">Edits to a subscription profile are overwritten on the next update.</p>
	{/if}
	<div class="editor"><YamlEditor bind:value={editing.content} /></div>
	{#snippet footer()}
		<Button onclick={() => (editorOpen = false)}>Cancel</Button>
		<Button variant="primary" loading={busy.save} disabled={!editing.name.trim()} onclick={saveEditor}>Save</Button>
	{/snippet}
</Dialog>

<style>
	.interval-control { display: flex; flex-direction: column; gap: var(--space-2); margin-top: var(--space-2); }
	.interval-input { display: flex; gap: var(--space-2); align-items: center; max-width: 230px; }
	.refresh-error { color: var(--color-danger); white-space: pre-wrap; overflow-wrap: anywhere; }
	.list {
		display: flex;
		flex-direction: column;
		gap: var(--space-3);
	}
	.profile {
		display: flex;
		align-items: flex-start;
		justify-content: space-between;
		gap: var(--space-4);
	}
	.info {
		display: flex;
		flex-direction: column;
		gap: var(--space-1);
		min-width: 0;
		flex: 1;
	}
	.title {
		display: flex;
		align-items: center;
		gap: var(--space-2);
	}
	.source {
		font-size: var(--text-xs);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.usage {
		display: flex;
		flex-direction: column;
		gap: var(--space-1);
		max-width: 360px;
		margin-top: var(--space-1);
	}
	.actions {
		display: flex;
		gap: var(--space-2);
		flex-shrink: 0;
	}
	.small {
		font-size: var(--text-xs);
	}
	.label {
		font-size: var(--text-sm);
		font-weight: var(--weight-medium);
	}
	.editor {
		flex: 1;
		min-height: 0;
	}
</style>
