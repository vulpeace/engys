<script lang="ts">
	import { onMount } from 'svelte';
	import { listen } from '@tauri-apps/api/event';
	import { ask } from '@tauri-apps/plugin-dialog';
	import { Plus, Pencil, Trash2, Save, Undo2, RefreshCw, ClipboardPaste, ArrowUp, ArrowDown } from '@lucide/svelte';
	import { parse, stringify } from 'yaml';
	import { Badge, Button, Card, Dialog, EmptyState, Field, PageHeader, Select, Switch, TextField, toast } from '#lib/ui/index.ts';
	import YamlEditor from '#lib/components/YamlEditor.svelte';
	import RuleConstructor from '#lib/components/RuleConstructor.svelte';
	import { backend, type RoutingView, type RoutingEntry } from '#lib/api/backend.ts';
	import { core } from '#lib/stores/core.svelte.ts';
	import { parsePastedRules, moveRule } from '#lib/rule-list.ts';

	let view = $state<RoutingView | null>(null);
	let entries = $state<RoutingEntry[]>([]);
	let providersYaml = $state('');
	let unlocked = $state(false);
	let replaceRules = $state(false);
	let loading = $state(false);
	let saving = $state(false);
	let error = $state('');
	let stale = $state(false);
	let query = $state('');
	let page = $state(0);
	let ruleOpen = $state(false);
	let editingId = $state<string | null>(null);
	let ruleText = $state('');
	let ruleTargets = $state<string[]>(['DIRECT', 'REJECT', 'GLOBAL']);
	const ruleProviders = $derived.by(() => {
		const names = new Set<string>();
		for (const yaml of [view?.inheritedProvidersYaml ?? '', providersYaml]) {
			try { const map = parse(yaml); if (map && typeof map === 'object' && !Array.isArray(map)) Object.keys(map).forEach((name) => names.add(name)); } catch { /* Keep inherited choices when custom YAML is incomplete. */ }
		}
		return [...names];
	});
	let position = $state('1');
	let pasteOpen = $state(false);
	let pastedText = $state('');
	let pastePosition = $state('1');
	const pasted = $derived.by(() => {
		try { return { rules: parsePastedRules(pastedText), error: '' }; }
		catch (e) { return { rules: [] as string[], error: String(e) }; }
	});
	const validPastePosition = $derived(Number.isInteger(Number(pastePosition)) && Number(pastePosition) >= 1 && Number(pastePosition) <= entries.length + 1);

	let providerOpen = $state(false);
	let providerName = $state('');
	let providerType = $state('http');
	let behavior = $state('domain');
	let format = $state('yaml');
	let providerUrl = $state('');
	let providerPath = $state('');
	let providerInterval = $state('86400');
	let payload = $state('');
	let providerError = $state('');
	const dirty = $derived(!!view && (replaceRules !== view.replaceRules || JSON.stringify(entries) !== JSON.stringify(view.entries) || providersYaml !== view.providersYaml));
	const inheritedDirty = $derived(!!view && JSON.stringify(entries.filter((e) => e.sourceIndex !== null)) !== JSON.stringify(view.entries.filter((e) => e.sourceIndex !== null)));
	const filtered = $derived(entries.map((entry, index) => ({ entry, index })).filter(({ entry }) => entry.rule.toLowerCase().includes(query.trim().toLowerCase())));
	const pages = $derived(Math.max(1, Math.ceil(filtered.length / 100)));
	const currentPage = $derived(Math.min(page, pages - 1));
	const rows = $derived(filtered.slice(currentPage * 100, (currentPage + 1) * 100));
	const canEdit = (e: RoutingEntry) => e.sourceIndex === null || unlocked;
	const validPosition = $derived(Number.isInteger(Number(position)) && Number(position) >= 1 && Number(position) <= entries.length + (editingId ? 0 : 1));

	function accept(next: RoutingView) {
		view = next; entries = structuredClone(next.entries); providersYaml = next.providersYaml;
		replaceRules = next.replaceRules;
		error = ''; stale = false; ruleOpen = false; pasteOpen = false; providerOpen = false; page = 0;
	}
	async function load() {
		loading = true;
		try { accept(await backend.routingGet()); }
		catch (e) { error = String(e); }
		finally { loading = false; }
	}
	async function reload() {
		if (dirty && !(await ask('Discard unsaved rule and provider changes?', { title: 'Reload Rules', kind: 'warning' }))) return;
		unlocked = false;
		await load();
	}
	onMount(() => {
		void load();
		const listeners = ['profiles-changed', 'routing-changed'].map((event) => listen(event, () => {
			if (saving) return;
			if (dirty || ruleOpen || pasteOpen || providerOpen) stale = true;
			else { unlocked = false; void load(); }
		}).catch(toast.error));
		return () => { for (const listener of listeners) void listener.then((stop) => stop?.()); };
	});
	async function save() {
		if (!view?.profileId) return;
		saving = true; error = '';
		try {
			accept(await backend.routingSave(view.profileId, view.revision, $state.snapshot(entries), providersYaml, unlocked, replaceRules));
			toast.success(core.running ? 'Rules saved and applied' : 'Rules saved for the next core start');
		} catch (e) { error = String(e); toast.error(e); }
		finally { saving = false; }
	}
	function edit(entry?: RoutingEntry, index = 0) {
		editingId = entry?.id ?? null; ruleText = entry?.rule ?? ''; position = String(index + 1); ruleOpen = true;
		void backend.customGroupsGet().then((context) => { ruleTargets = [...new Set(['DIRECT', 'REJECT', 'GLOBAL', ...context.members])]; }).catch(toast.error);
	}
	function putRule() {
		if (!validPosition) return;
		const old = entries.find((e) => e.id === editingId);
		if (old && !canEdit(old)) return;
		const next = entries.filter((e) => e.id !== editingId);
		next.splice(Number(position) - 1, 0, { id: editingId ?? crypto.randomUUID(), sourceIndex: old?.sourceIndex ?? null, rule: ruleText.trim() });
		entries = next; ruleOpen = false;
	}
	function openPaste(index = 0) {
		pastedText = ''; pastePosition = String(index + 1); pasteOpen = true;
	}
	function pasteRules() {
		if (!validPastePosition || pasted.error || !pasted.rules.length || stale || saving || entries.length + pasted.rules.length > 100000) return;
		const index = Number(pastePosition) - 1;
		entries = [...entries.slice(0, index), ...pasted.rules.map((rule) => ({ id: crypto.randomUUID(), sourceIndex: null, rule })), ...entries.slice(index)];
		pasteOpen = false;
	}
	function resetInherited() {
		if (!view) return;
		if (replaceRules) {
			replaceRules = false;
			entries = view.sourceRules.map((rule, sourceIndex) => ({ id: `source-${sourceIndex}`, sourceIndex, rule }));
			return;
		}
		const custom = new Map<number, RoutingEntry[]>();
		for (let i = 0; i < entries.length; i++) {
			if (entries[i].sourceIndex !== null) continue;
			const anchor = entries.slice(i + 1).find((e) => e.sourceIndex !== null)?.sourceIndex ?? view.sourceRules.length;
			custom.set(anchor, [...(custom.get(anchor) ?? []), entries[i]]);
		}
		entries = view.sourceRules.flatMap((rule, i) => [...(custom.get(i) ?? []), { id: `source-${i}`, sourceIndex: i, rule }]);
		entries.push(...(custom.get(view.sourceRules.length) ?? []));
	}
	function addProvider() {
		providerError = '';
		try {
			const current = parse(providersYaml) ?? {};
			const inherited = parse(view?.inheritedProvidersYaml ?? '') ?? {};
			const name = providerName.trim();
			if (!current || typeof current !== 'object' || Array.isArray(current)) throw new Error('Custom providers must be a YAML mapping.');
			if (!name || Object.hasOwn(current, name) || Object.hasOwn(inherited, name)) throw new Error('Choose a unique provider name.');
			const definition: Record<string, unknown> = { type: providerType, behavior };
			if (providerType === 'inline') definition.payload = payload.split('\n').map((s) => s.trim()).filter(Boolean);
			else {
				definition.format = behavior === 'classical' && format === 'mrs' ? 'yaml' : format;
				if (providerType === 'http') {
					const url = new URL(providerUrl);
					if (!['https:', 'http:'].includes(url.protocol)) throw new Error('Enter an HTTP(S) URL.');
					if (!Number.isInteger(Number(providerInterval)) || Number(providerInterval) < 0) throw new Error('Interval must be a nonnegative number of seconds.');
					definition.url = providerUrl.trim(); definition.interval = Number(providerInterval);
				} else if (!providerPath.trim()) throw new Error('Enter a provider file path.');
				if (providerPath.trim()) definition.path = providerPath.trim();
			}
			Object.defineProperty(current, name, { value: definition, enumerable: true, configurable: true, writable: true });
			providersYaml = stringify(current); providerOpen = false;
		} catch (e) { providerError = String(e); }
	}
	function newProvider() {
		providerName = ''; providerType = 'http'; behavior = 'domain'; format = 'yaml'; providerUrl = ''; providerPath = ''; providerInterval = '86400'; payload = ''; providerError = ''; providerOpen = true;
	}
</script>

<PageHeader title="Rules" subtitle={view?.profileName ? `Rules and rule-providers for ${view.profileName}` : 'Subscription rules and custom routing'}>
	{#snippet actions()}
		<Button disabled={loading || saving} onclick={reload}><RefreshCw size={14} /> Reload</Button>
		<Button disabled={!view?.profileId || loading || saving || stale} onclick={() => edit()}><Plus size={14} /> Add rule</Button>
		<Button disabled={!view?.profileId || loading || saving || stale} onclick={() => openPaste()}><ClipboardPaste size={14} /> Paste rules</Button>
		<Button variant="primary" loading={saving} disabled={!view?.profileId || !dirty || loading || stale} onclick={save}><Save size={14} /> Save & apply</Button>
	{/snippet}
</PageHeader>
{#if error}<p class="error" role="alert">{error}</p>{/if}
{#if stale}<Card><p>The active profile or its rules changed. Reload to use the latest subscription before saving. Your draft is kept until you reload.</p></Card>{/if}
{#if view?.profileId}
	<Card>
		<Field label="Allow editing inherited rules" description="Unlock to edit or remove subscription rules. Unlock also allows changing their order. Save inherited edits before locking again. This toggle starts locked each time you open this tab.">
			<Switch label="Allow editing inherited rules" bind:checked={unlocked} disabled={saving || loading || (inheritedDirty && unlocked)} />
		</Field>
		{#if replaceRules}
			<p class="hint">This profile uses a saved rule list in place of the source rules.</p>
			<Button disabled={saving} onclick={resetInherited}><Undo2 size={14} /> Use subscription rules</Button>
		{:else if unlocked}
			<Button disabled={saving} onclick={resetInherited}><Undo2 size={14} /> Reset inherited rules</Button>
		{/if}
		<p class="hint">First matching rule wins. Paste one rule per line, insert before or after any row, and use the arrows or position field to move rules. Positions refer to the full list, including search results on other pages. Rules after MATCH are usually unreachable. Proxied Apps rules are added separately according to that tab's ordering toggle.</p>
		<p class="hint">To transfer these rules to another profile, duplicate this profile from the Profiles tab.</p>
		{#if dirty}<p class="hint">Unsaved changes. Save & apply validates the complete config with the installed core.</p>{/if}
	</Card>
	<Card title={`Rules (${entries.length})`}>
		<TextField aria-label="Search rules" placeholder="Search rules…" bind:value={query} oninput={() => page = 0} disabled={saving} />
		<div class="rule-list" aria-busy={saving}>
			{#each rows as { entry, index } (entry.id)}
				<div class="rule-row">
					<span class="number">{index + 1}</span>
					<div class="definition"><code>{entry.rule}</code><div class="tags"><Badge>{entry.sourceIndex === null ? 'Custom' : entry.rule !== view.sourceRules[entry.sourceIndex] ? 'Inherited · modified' : 'Inherited'}</Badge></div></div>
					<div class="row-actions">
						<Button icon size="sm" title="Move rule up" disabled={saving || stale || index === 0 || !canEdit(entry)} onclick={() => entries = moveRule(entries, index, index - 1, unlocked)}><ArrowUp size={14} /></Button>
						<Button icon size="sm" title="Move rule down" disabled={saving || stale || index === entries.length - 1 || !canEdit(entry)} onclick={() => entries = moveRule(entries, index, index + 1, unlocked)}><ArrowDown size={14} /></Button>
						<Button icon size="sm" title="Insert rule before" disabled={saving || stale} onclick={() => edit(undefined, index)}><Plus size={14} /><span class="before">↑</span></Button>
						<Button icon size="sm" title="Insert rule after" disabled={saving || stale} onclick={() => edit(undefined, index + 1)}><Plus size={14} /><span class="after">↓</span></Button>
						<Button icon size="sm" title={canEdit(entry) ? 'Edit rule' : 'Unlock inherited editing to edit this rule'} disabled={saving || stale || !canEdit(entry)} onclick={() => edit(entry, index)}><Pencil size={14} /></Button>
						<Button icon size="sm" variant="danger" title="Delete rule" disabled={saving || stale || !canEdit(entry)} onclick={() => entries = entries.filter((e) => e.id !== entry.id)}><Trash2 size={14} /></Button>
					</div>
				</div>
			{:else}<p class="hint">{query ? 'No matching rules.' : 'No rules yet. Add a rule to begin.'}</p>{/each}
		</div>
		<div class="pagination"><Button disabled={currentPage === 0} onclick={() => page = currentPage - 1}>Previous</Button><span>{currentPage + 1} / {pages}</span><Button disabled={currentPage >= pages - 1} onclick={() => page = currentPage + 1}>Next</Button></div>
	</Card>
	<Card title="Inherited rule-providers"><p class="hint">Defined by the subscription; shown read-only. RULE-SET entries refer to these provider names.</p><pre class="provider-source">{view.inheritedProvidersYaml}</pre></Card>
	<Card title="Custom rule-providers">
		<p class="hint">Add a provider, then insert a rule such as RULE-SET,ProviderName,DIRECT at the desired position. Edit or remove custom definitions below. HTTP intervals use seconds; file paths resolve under the core working directory and follow mihomo's path restrictions.</p>
		<Button disabled={saving || stale} onclick={newProvider}><Plus size={14} /> Add provider</Button>

		{#if !saving}
		<div class="rules-yaml">
			<YamlEditor bind:value={providersYaml} />
		</div>
		{:else}<pre class="provider-source">{providersYaml}</pre>{/if}
	</Card>
{:else if !loading}<EmptyState title="No active profile" description="Choose a profile on Profiles to view and customize its rules." />{/if}

<Dialog bind:open={ruleOpen} title={editingId ? 'Edit rule' : 'Insert custom rule'} description="Use mihomo rule syntax, for example DOMAIN-SUFFIX,example.com,DIRECT or RULE-SET,MyProvider,MyGroup.">
	{#if ruleOpen}<RuleConstructor bind:value={ruleText} targets={ruleTargets} providers={ruleProviders} />{/if}
	{#if !editingId || unlocked || entries.find((e) => e.id === editingId)?.sourceIndex === null}
		<Field label="Position in full list (1-based)" stacked id="rule-position"><TextField id="rule-position" type="number" min="1" max={entries.length + (editingId ? 0 : 1)} step="1" bind:value={position} /></Field>
	{/if}
	{#snippet footer()}<Button onclick={() => ruleOpen = false}>Cancel</Button><Button variant="primary" disabled={!ruleText.trim() || !validPosition || saving || stale} onclick={putRule}>Keep in draft</Button>{/snippet}
</Dialog>
<Dialog bind:open={pasteOpen} title="Paste rules" description="Paste one mihomo rule per line, or a YAML rules list. The pasted order is preserved; blank lines and full-line comments are ignored.">
	<Field label="Rules" stacked><textarea aria-label="Rules to paste" bind:value={pastedText} rows="10" placeholder={'DOMAIN-SUFFIX,example.com,DIRECT\nRULE-SET,MyProvider,MyGroup\nIP-CIDR,192.0.2.0/24,REJECT,no-resolve'}></textarea></Field>
	<Field label="Insert at position (1-based)" stacked id="paste-position"><TextField id="paste-position" type="number" min="1" max={entries.length + 1} step="1" bind:value={pastePosition} /></Field>
	{#if pasted.error}<p class="error" role="alert">{pasted.error}</p>{:else}<p class="hint">{pasted.rules.length} rules ready to insert. Save & apply will validate them with mihomo.</p>{/if}
	{#if entries.length + pasted.rules.length > 100000}<p class="error" role="alert">At most 100,000 rules are supported.</p>{/if}
	{#snippet footer()}<Button onclick={() => pasteOpen = false}>Cancel</Button><Button variant="primary" disabled={!pasted.rules.length || !!pasted.error || !validPastePosition || saving || stale || entries.length + pasted.rules.length > 100000} onclick={pasteRules}>Insert into draft</Button>{/snippet}
</Dialog>
<Dialog bind:open={providerOpen} title="Add custom rule-provider" description="Adding a provider alone does not change routing. Insert a RULE-SET rule to use it.">
	<Field label="Name" stacked id="provider-name"><TextField id="provider-name" bind:value={providerName} placeholder="MyProvider" /></Field>
	<Field label="Source" stacked><Select label="Provider source" bind:value={providerType} options={['http', 'file', 'inline'].map((value) => ({ value, label: value }))} /></Field>
	<Field label="Behavior" stacked><Select label="Provider behavior" bind:value={behavior} options={['domain', 'ipcidr', 'classical'].map((value) => ({ value, label: value }))} /></Field>
	{#if providerType !== 'inline'}<Field label="Format" stacked><Select label="Provider format" bind:value={format} options={(behavior === 'classical' ? ['yaml', 'text'] : ['yaml', 'text', 'mrs']).map((value) => ({ value, label: value }))} /></Field>{/if}
	{#if providerType === 'http'}
		<Field label="URL" stacked id="provider-url"><TextField id="provider-url" bind:value={providerUrl} type="url" /></Field>
		<Field label="Update interval (seconds; 0 disables)" stacked id="provider-interval"><TextField id="provider-interval" type="number" min="0" step="1" bind:value={providerInterval} /></Field>
	{/if}
	{#if providerType !== 'inline'}<Field label={providerType === 'file' ? 'File path' : 'Cache path (optional)'} stacked id="provider-path"><TextField id="provider-path" bind:value={providerPath} placeholder="rules/my-provider.yaml" /></Field>{/if}
	{#if providerType === 'inline'}<Field label="Payload (one entry per line)" stacked><textarea aria-label="Inline provider payload" bind:value={payload} rows="6" placeholder={behavior === 'classical' ? 'DOMAIN-SUFFIX,example.com' : behavior === 'ipcidr' ? '192.0.2.0/24' : '+.example.com'}></textarea></Field>{/if}
	{#if providerError}<p class="error" role="alert">{providerError}</p>{/if}
	{#snippet footer()}<Button onclick={() => providerOpen = false}>Cancel</Button><Button variant="primary" disabled={!providerName.trim()} onclick={addProvider}>Add to draft</Button>{/snippet}
</Dialog>

<style>
	.hint { color: var(--color-text-muted); font-size: var(--text-xs); line-height: 1.6; }
	.error { color: var(--color-danger); white-space: pre-wrap; overflow-wrap: anywhere; }
	.rule-row { display: flex; align-items: center; gap: var(--space-3); padding: var(--space-3) 0; border-bottom: 1px solid var(--color-border); }
	.number { min-width: 3ch; color: var(--color-text-muted); font-size: var(--text-xs); }
	.definition { flex: 1; min-width: 0; }
	code { overflow-wrap: anywhere; user-select: text; font-size: var(--text-xs); }
	.tags { margin-top: var(--space-1); }
	.row-actions, .pagination { display: flex; align-items: center; gap: var(--space-1); }
	.pagination { justify-content: flex-end; gap: var(--space-3); margin-top: var(--space-3); font-size: var(--text-xs); }
	.provider-source { max-height: 280px; overflow: auto; font-size: var(--text-xs); user-select: text; white-space: pre-wrap; overflow-wrap: anywhere; }
	.rules-yaml { height: fit-content; }
	textarea { width: 100%; padding: var(--space-2); background: var(--color-surface); color: var(--color-text); border: 1px solid var(--color-border); border-radius: var(--radius-md); font-family: var(--font-mono); }
	.after { font-size: var(--text-xs); }
</style>
