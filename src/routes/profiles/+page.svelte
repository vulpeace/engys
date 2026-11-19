<script lang="ts">
	import { onMount } from 'svelte';
	import { listen } from '@tauri-apps/api/event';
	import { Copy, Pencil, RefreshCw, Trash2, Check, Undo2, Eye } from '@lucide/svelte';
	import { ask } from '@tauri-apps/plugin-dialog';
	import { Badge, Button, Card, Dialog, EmptyState, Meter, PageHeader, TextField, toast } from '#lib/ui/index.ts';
	import ProfileActions from '#lib/components/ProfileActions.svelte';
	import SourceRuleChoiceDialog from '#lib/components/SourceRuleChoiceDialog.svelte';
	import YamlEditor from '#lib/components/YamlEditor.svelte';
	import { backend, type Profile, type ProfileIndex, type SourceRuleChoice } from '#lib/api/backend.ts';
	import { core } from '#lib/stores/core.svelte.ts';
	import { validProfileInterval as validInterval } from '#lib/profile-interval.ts';
	import { bytes, dateTime, timeAgo } from '#lib/format.ts';


	let index = $state<ProfileIndex>({ active: null, profiles: [] });
	let busy = $state<Record<string, boolean>>({});

	// Dialog state
	let intervalDrafts = $state<Record<string, string>>({});
	let previewOpen = $state(false);
	let preview = $state({ name: '', content: '' });
	let editorOpen = $state(false);
	let editing = $state({ id: '', name: '', content: '', revision: '' });
	let pendingChoices = $state.raw<{ name: string; resolve: (choice: SourceRuleChoice | null) => void }[]>([]);
	let mounted = false;

	async function refresh() {
		index = await backend.profilesList();
		await core.refreshSettings();
	}
	refresh().catch(toast.error);
	onMount(() => {
		mounted = true;
		const unlisten = listen<ProfileIndex>('profiles-changed', (event) => { index = event.payload; }).catch(toast.error);
		return () => {
			mounted = false;
			for (const pending of pendingChoices) pending.resolve(null);
			pendingChoices = [];
			void unlisten.then((stop) => stop?.());
		};
	});

	function chooseRules(name: string): Promise<SourceRuleChoice | null> {
		if (!mounted) return Promise.resolve(null);
		return new Promise((resolve) => { pendingChoices = [...pendingChoices, { name, resolve }]; });
	}

	function finishChoice(choice: SourceRuleChoice | null) {
		const [pending, ...remaining] = pendingChoices;
		pendingChoices = remaining;
		pending?.resolve(choice);
	}

	async function run(key: string, fn: () => Promise<unknown>, ok?: string) {
		busy[key] = true;
		try {
			if (await fn() === false) return false;
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

	async function edit(p: Profile) {
		try {
			const content = await backend.profileRead(p.id);
			editing = { id: p.id, name: p.name, content, revision: content };
			editorOpen = true;
		} catch (e) {
			toast.error(e);
		}
	}

	async function viewFinal(p: Profile) {
		busy[`preview:${p.id}`] = true;
		try {
			preview = { name: p.name, content: await backend.profileReadFinal(p.id) };
			previewOpen = true;
		} catch (e) {
			toast.error(e);
		} finally {
			busy[`preview:${p.id}`] = false;
		}
	}

	async function saveEditor() {
		const { id, name, content, revision } = editing;
		const ok = await run('save', async () => {
			if (!(await backend.profileSave(id, revision, content))) {
				const choice = await chooseRules(name);
				if (!choice || !(await backend.profileSave(id, revision, content, choice))) return false;
			}
			editing.revision = content;
			const current = index.profiles.find((p) => p.id === id);
			if (current && current.name !== name) await backend.profileRename(id, name);
		}, 'Profile saved');
		if (ok) editorOpen = false;
	}

	const activate = (p: Profile) =>
		run(`activate:${p.id}`, () => backend.profileActivate(p.id), core.running ? `Switched to ${p.name}` : undefined);

	const update = (p: Profile) =>
		run(`update:${p.id}`, async () => !!(await backend.profileUpdateRemote(p.id, () => chooseRules(p.name))), `Updated ${p.name}`);

	const duplicate = (p: Profile) =>
		run(`duplicate:${p.id}`, () => backend.profileDuplicate(p.id), `Duplicated ${p.name}`);

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
		<ProfileActions onadded={refresh} />
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
				<div class="profile" class:active class:android={core.android}>
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
						<Button size="sm" icon title="View final config" aria-label="View final config" loading={busy[`preview:${p.id}`]} onclick={() => viewFinal(p)}><Eye size={14} /></Button>
						<Button size="sm" icon title="Edit" onclick={() => edit(p)}><Pencil size={14} /></Button>
						<Button size="sm" icon title="Duplicate profile" aria-label="Duplicate profile" loading={busy[`duplicate:${p.id}`]} onclick={() => duplicate(p)}><Copy size={14} /></Button>
						<Button size="sm" icon variant="danger" title="Delete" loading={busy[`delete:${p.id}`]} onclick={() => remove(p)}>
							<Trash2 size={14} />
						</Button>
					</div>
				</div>
			</Card>
		{/each}
	</div>
{/if}

<Dialog bind:open={previewOpen} wide title={`Final config — ${preview.name}`} description="Read-only config generated from this profile with the current settings, rules, and custom proxy groups.">
	<div class="editor"><YamlEditor value={preview.content} readOnly /></div>
	{#snippet footer()}
		<Button onclick={() => (previewOpen = false)}>Close</Button>
	{/snippet}
</Dialog>

<Dialog bind:open={editorOpen} wide title="Edit profile">
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

<SourceRuleChoiceDialog name={pendingChoices[0]?.name} onchoice={finishChoice} />

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
		flex-wrap: wrap;
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

	.editor {
		flex: 1;
		min-height: 0;
	}
    .title strong { min-width: 0; overflow-wrap: anywhere; }
    .profile.android { flex-direction: column; }
    .profile.android .info { width: 100%; flex: auto; }
    .profile.android .actions { flex-wrap: wrap; max-width: 100%; }
</style>
