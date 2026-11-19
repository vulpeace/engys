<script lang="ts">
    import InstalledAppIcon from '#lib/components/InstalledAppIcon.svelte';
    import { installedApps } from '#lib/stores/installed-apps.ts';
	import { onMount } from 'svelte';
	import { listen } from '@tauri-apps/api/event';
	import { AppWindow, FolderOpen, ListPlus, Trash2, Info, Plus, Save, Undo2, Pencil, ArrowUp, ArrowDown } from '@lucide/svelte';
	import { ask, open as openFile } from '@tauri-apps/plugin-dialog';
	import { Badge, Button, Card, Dialog, EmptyState, Field, PageHeader, Select, Switch, TextField, toast } from '#lib/ui/index.ts';
	import { backend, type AppRule, type AppMatchKind, type AppRulesScope, type AppRulesView, type ProcessEntry } from '#lib/api/backend.ts';
	import { core } from '#lib/stores/core.svelte.ts';

	const MATCHES: { value: AppMatchKind; label: string }[] = $derived(core.android ? [{ value: 'processName', label: 'Package name' }, { value: 'processNameRegex', label: 'Package regex' }] : [
		{ value: 'processName', label: 'Process name' },
		{ value: 'processPath', label: 'Executable path' },
		{ value: 'processNameRegex', label: 'Name regex' },
		{ value: 'processPathRegex', label: 'Path regex' }
	]);
	const RULE_TYPES: Record<AppMatchKind, string> = {
		processName: 'PROCESS-NAME', processPath: 'PROCESS-PATH',
		processNameRegex: 'PROCESS-NAME-REGEX', processPathRegex: 'PROCESS-PATH-REGEX'
	};
	const STORAGE_KEY = 'proxied-apps-mockup';
	let view = $state.raw<AppRulesView | null>(null);
	let scope = $state<AppRulesScope>('global');
	let scopeSelection = $state('global');
	let changingScope = $state(false);
	let apps = $state<AppRule[]>([]);
	let orderOverride = $state<boolean | null>(null);
	const targets = $derived(view?.targets ?? ['GLOBAL', 'DIRECT', 'REJECT']);
	const activeProfile = $derived(view?.activeProfile ?? null);
	const inferredSelectThroughProxy = $derived(view?.inferredSelectThroughProxy ?? null);
	const selectThroughProxy = $derived(orderOverride ?? inferredSelectThroughProxy ?? false);
	let loading = $state(true);
	let saving = $state(false);
	let stale = $state(false);
	let loadRequest = 0;
	let error = $state('');
	let legacy = $state<AppRule[]>([]);
	let legacyImported = $state(false);
	const blocked = $derived(!view || loading || saving || stale);
	const scopes = $derived([
		{ value: 'global', label: 'Global' },
		{ value: 'profile', label: 'This profile', disabled: !activeProfile }
	]);
	const rulesFor = (saved: AppRulesView, selected: AppRulesScope) =>
		selected === 'profile' ? saved.profileRules ?? saved.globalRules : saved.globalRules;
	const rulesDirty = $derived(!!view && JSON.stringify(apps) !== JSON.stringify(rulesFor(view, scope)));
	const dirty = $derived(!!view && (rulesDirty || orderOverride !== view.orderOverride || scope !== view.scope));
	const unavailable = $derived(apps.some((a) => !targets.includes(a.target)));
	const targetOptions = $derived([
		...targets.map((name) => ({ value: name, label: name === 'GLOBAL' ? 'Proxy (GLOBAL)' : name === 'DIRECT' ? 'Direct' : name === 'REJECT' ? 'Block' : name })),
		...[...new Set(apps.map((a) => a.target))].filter((name) => !targets.includes(name))
			.map((name) => ({ value: name, label: `${name} (unavailable)`, disabled: true }))
	]);
	const defaultTarget = $derived(selectThroughProxy ? targets.find((t) => !['GLOBAL', 'DIRECT', 'REJECT'].includes(t)) ?? 'GLOBAL' : 'DIRECT');

	let pickerOpen = $state(false);
	let processes = $state<ProcessEntry[]>([]);
	let loadingProcs = $state(false);
	let processError = $state('');
	let query = $state('');
	let showSystem = $state(false);
	const pickerKind = (p: ProcessEntry): AppMatchKind => p.packageName ? 'processName' : p.path ? 'processPath' : 'processName';
	const pickerValue = (p: ProcessEntry) => p.packageName ?? p.path ?? p.name;
	let manualOpen = $state(false);
	let editingId = $state<string | null>(null);
	let matchKind = $state<AppMatchKind>('processName');
	let matchValue = $state('');
	let matchTarget = $state('GLOBAL');
	let formError = $state('');
	const regex = $derived(matchKind.endsWith('Regex'));
	const filtered = $derived.by(() => {
		const q = query.trim().toLowerCase();
		return processes.filter((p) => (!core.android || showSystem || !p.system) && (!q || p.name.toLowerCase().includes(q) || (p.packageName ?? p.path)?.toLowerCase().includes(q)));
	});
	const ruleFor = (a: AppRule) => `${RULE_TYPES[a.kind]},${a.value},${a.target}`;
	const titleFor = (a: AppRule) => a.kind === 'processPath' ? a.value.split(/[\\/]/).pop() ?? a.value : a.value;
	const sameMatch = (a: AppRule, kind: AppMatchKind, value: string) =>
		a.kind === kind && (kind.endsWith('Regex') ? a.value === value : a.value.toLowerCase() === value.toLowerCase());

	function accept(next: AppRulesView) {
		view = next;
		scope = next.scope;
		scopeSelection = scope;
		apps = structuredClone(rulesFor(next, scope));
		orderOverride = next.orderOverride;
		error = '';
		stale = false;
		legacyImported = false;
		manualOpen = false;
		pickerOpen = false;
	}

	async function load(refresh = false) {
		const request = ++loadRequest;
		if (!refresh) loading = true;
		try {
			const next = await backend.appRulesGet();
			if (request !== loadRequest) return;
			if (refresh && view) {
				if (view.activeProfile === next.activeProfile) {
					view = next;
					stale = false;
					return;
				}
				if (dirty || manualOpen || pickerOpen) {
					stale = true;
					return;
				}
			}
			accept(next);
		} catch (e) {
			if (request === loadRequest) error = String(e);
		} finally {
			if (request === loadRequest) loading = false;
		}
	}

	async function changeScope(value: string) {
		changingScope = true;
		try {
			if (!view || blocked || value === scope || (value !== 'global' && value !== 'profile')) return;
			if (value === 'profile' && !activeProfile) return;
			const profile = activeProfile;
			if (rulesDirty && !(await ask('Discard unsaved application rule changes?', { title: 'Change rule scope', kind: 'warning' }))) return;
			if (blocked || !view || activeProfile !== profile) return;
			scope = value;
			apps = structuredClone(rulesFor(view, scope));
			legacyImported = false;
			error = '';
		} catch (e) { toast.error(e); }
		finally {
			scopeSelection = scope;
			changingScope = false;
		}
	}

	async function reload() {
		if (dirty && !(await ask('Discard unsaved application rule changes?', { title: 'Reload Proxied apps', kind: 'warning' }))) return;
		await load();
	}

	onMount(() => {
		const listeners = ['profiles-changed', 'custom-groups-changed', 'routing-changed'].map((event) => listen(event, () => {
			if (!loading && !saving) void load(true);
		}).catch(toast.error));
		void load();
		return () => { for (const listener of listeners) void listener.then((stop) => stop?.()); };
	});

	// Offer an explicit draft import; old preview-only rules never activate on their own.
	try {
		const old: unknown = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? '[]');
		if (Array.isArray(old)) {
			legacy = old.flatMap((item) => {
				if (!item || typeof item !== 'object' || typeof item.name !== 'string' ||
					!['proxy', 'direct', 'reject'].includes(item.action)) return [];
				const path = typeof item.path === 'string' && item.path ? item.path : null;
				return [{ id: crypto.randomUUID(), kind: path ? 'processPath' : 'processName', value: path ?? item.name,
					target: item.action === 'direct' ? 'DIRECT' : item.action === 'reject' ? 'REJECT' : 'GLOBAL' } satisfies AppRule];
			});
		}
	} catch { /* No valid previous selections. */ }

	function importLegacy() {
		for (const rule of legacy) {
			if (!apps.some((a) => sameMatch(a, rule.kind, rule.value))) apps.push(rule);
		}
		legacyImported = true;
	}

	async function save() {
		if (blocked || !dirty || unavailable) return;
		saving = true;
		++loadRequest;
		error = '';
		try {
			const next = await backend.appRulesSave(activeProfile, scope, $state.snapshot(apps), orderOverride);
			if (legacyImported) {
				try { localStorage.removeItem(STORAGE_KEY); } catch { /* Storage unavailable. */ }
				legacy = [];
			}
			accept(next);
			toast.success(core.settings?.mode !== 'rule' ? 'Rules saved. Switch to Rule mode to use them.' : core.running ? 'Application rules saved and applied to new connections' : 'Application rules saved for the next core start');
		} catch (e) {
			error = String(e);
			toast.error(e);
		} finally {
			saving = false;
			void load(true);
		}
	}

	function add(kind: AppMatchKind, value: string, target = defaultTarget) {
		if (apps.some((a) => sameMatch(a, kind, value))) {
			toast.info('This application match is already in the list');
			return false;
		}
		apps.push({ id: crypto.randomUUID(), kind, value, target });
		return true;
	}

	function edit(rule?: AppRule) {
		editingId = rule?.id ?? null;
		matchKind = rule?.kind ?? 'processName';
		matchValue = rule?.value ?? '';
		matchTarget = rule?.target ?? defaultTarget;
		formError = '';
		manualOpen = true;
	}

	function submitManual() {
		const value = matchValue.trim();
		if (!value) { formError = 'Enter a process name, executable path, or regex.'; return; }
		if (apps.some((a) => a.id !== editingId && sameMatch(a, matchKind, value))) {
			formError = 'This application match is already in the list.'; return;
		}
		if (editingId) {
			apps = apps.map((a) => a.id === editingId ? { ...a, kind: matchKind, value, target: matchTarget } : a);
		} else { add(matchKind, value, matchTarget); }
		manualOpen = false;
	}

	function move(index: number, delta: number) {
		const reordered = [...apps];
		[reordered[index], reordered[index + delta]] = [reordered[index + delta], reordered[index]];
		apps = reordered;
	}

	async function openPicker() {
		pickerOpen = true;
		loadingProcs = true;
		processError = '';
		try { processes = core.android ? await installedApps() : await backend.processesList(); }
		catch (e) { processError = String(e); }
		finally { loadingProcs = false; }
	}

	async function browse() {
		try {
			const path = await openFile({ multiple: false, directory: false, title: 'Choose an executable' });
			if (typeof path === 'string') add('processPath', path);
		} catch (e) { toast.error(e); }
	}

	async function useRuleMode() {
		try { await core.setMode('rule'); } catch (e) { toast.error(e); }
	}
</script>

<PageHeader title="Proxied apps" subtitle="Route applications through a proxy, directly, or block them">
	{#snippet actions()}
		<Select options={scopes} bind:value={scopeSelection} onValueChange={changeScope} disabled={blocked || changingScope} label="Application rule scope" />
		<Button disabled={blocked || !dirty} onclick={() => { if (view) accept(view); }}><Undo2 size={14} /> Revert</Button>
		<Button variant="primary" disabled={blocked || !dirty || unavailable} loading={saving} onclick={save}><Save size={14} /> Save & apply</Button>
	{/snippet}
</PageHeader>

<Card>
	<div class="notice"><Info size={16} /><div>
		<p>{selectThroughProxy ? 'Application rules follow the profile’s rules, just before its catch-all. Earlier profile rules take priority.' : 'Application rules go before the profile’s rules and take priority.'} Within the app list, the first match wins. Click Save & apply to activate changes for new connections.</p>
		<p>Only traffic reaching mihomo can be routed. Enable TUN to capture applications that don't use its proxy port.</p>
	</div></div>
	<Field label="Select through proxy" description="On: put app rules last, before the catch-all, and default new apps to Proxy. Off: put them first and default new apps to Direct. Existing route choices are preserved.">
		<Switch label="Select through proxy" checked={selectThroughProxy} disabled={blocked} onCheckedChange={(value) => orderOverride = value} />
	</Field>
	<div class="toolbar">
		<span class="muted note">{orderOverride !== null ? 'Custom order for this profile.' : inferredSelectThroughProxy === null ? 'Fallback is ambiguous; application rules default to first.' : inferredSelectThroughProxy ? 'Automatic: Direct fallback detected.' : 'Automatic: Proxy fallback detected.'}</span>
		{#if orderOverride !== null}<Button size="sm" disabled={blocked} onclick={() => orderOverride = null}>Use profile default</Button>{/if}
	</div>
	{#if core.settings?.mode !== 'rule'}
		<div class="mode-notice"><span>Application rules are inactive in {core.settings?.mode ?? 'the current'} mode.</span><Button size="sm" onclick={useRuleMode}>Use Rule mode</Button></div>
	{/if}
	{#if !loading && !activeProfile}<p class="muted note">No active profile. Built-in targets can be saved now; select a profile before starting the core.</p>{/if}
	{#if unavailable}<p class="error" role="alert">Some targets are missing from the active profile. Choose available targets before saving.</p>{/if}
</Card>

{#if stale}
	<Card><p class="error" role="alert">The active profile changed while you were editing.</p><Button disabled={loading} onclick={() => void reload().catch(toast.error)}>Load active profile</Button></Card>
{/if}
{#if legacy.length && !legacyImported}
	<Card><div class="toolbar"><span class="muted">Previous preview selections are available.</span><Button disabled={blocked} onclick={importLegacy}>Import previous selections</Button></div></Card>
{/if}
{#if error}<Card><p class="error selectable" role="alert">{error}</p>{#if !dirty}<Button disabled={loading} onclick={() => void load()}>Retry</Button>{/if}</Card>{/if}

<Card title="Applications" padded={false}>
	<div class="toolbar add-tools">
		<Button disabled={blocked} onclick={openPicker}><ListPlus size={14} /> {core.android ? 'Installed apps' : 'From running apps'}</Button>
		{#if !core.android}<Button disabled={blocked} onclick={browse}><FolderOpen size={14} /> Browse executable</Button>{/if}
		<Button disabled={blocked} onclick={() => edit()}><Plus size={14} /> Add manually</Button>
	</div>
	{#if loading}<p class="muted loading">Loading application rules…</p>
	{:else if !apps.length}<EmptyState title="No applications" description={core.android ? "Choose an installed application or add a package name manually." : "Pick a running application, browse to an executable, or add a name or regex manually."} />
	{:else}
		<ul class="apps">
			{#each apps as app, i (app.id)}
				<li>
					<AppWindow size={18} />
					<div class="app-info"><strong class="path" title={app.value}>{titleFor(app)}</strong><span class="muted kind">{MATCHES.find((m) => m.value === app.kind)?.label}</span>{#if app.kind === 'processPath'}<span class="muted mono path" title={app.value}>{app.value}</span>{/if}</div>
					<Select options={targetOptions} bind:value={app.target} disabled={blocked} label="Route for {titleFor(app)}" />
					<div class="row-actions">
						<Button icon size="sm" disabled={blocked || i === 0} title="Move rule up" onclick={() => move(i, -1)}><ArrowUp size={14} /></Button>
						<Button icon size="sm" disabled={blocked || i === apps.length - 1} title="Move rule down" onclick={() => move(i, 1)}><ArrowDown size={14} /></Button>
						<Button icon size="sm" disabled={blocked} title="Edit application rule" onclick={() => edit(app)}><Pencil size={14} /></Button>
						<Button icon size="sm" variant="danger" disabled={blocked} title="Remove application rule" onclick={() => apps.splice(i, 1)}><Trash2 size={14} /></Button>
					</div>
				</li>
			{/each}
		</ul>
	{/if}
</Card>

{#if apps.length}<Card title="Generated rules"><pre class="rules selectable">{apps.map((a) => `- ${JSON.stringify(ruleFor(a))}`).join('\n')}</pre></Card>{/if}

<Dialog bind:open={manualOpen} title={editingId ? 'Edit application rule' : 'Add application rule'} description={core.android ? "Match an Android package name or package-name regex." : "Match an executable by its name, full path, or a regular expression."}>
	<Field label="Match" stacked><Select options={MATCHES} bind:value={matchKind} label="Match type" /></Field>
	<Field label={regex ? 'Regular expression' : matchKind === 'processPath' ? 'Executable path' : core.android ? 'Package name' : 'Process name'} stacked id="app-match">
		<TextField id="app-match" bind:value={matchValue} placeholder={regex ? '^(firefox|chrome)(\\.exe)?$' : matchKind === 'processPath' ? '/usr/bin/firefox' : core.android ? 'org.mozilla.firefox' : 'firefox.exe'} onkeydown={(e) => { if (e.key === 'Enter') submitManual(); }} />
	</Field>
	{#if regex}<p class="muted note">Regexes match names or paths case-insensitively. Use ^ and $ for a whole match. The installed mihomo core validates syntax when you save.</p>{/if}
	<Field label="Route to" stacked><Select options={targetOptions} bind:value={matchTarget} label="Routing target" /></Field>
	{#if formError}<p class="error" role="alert">{formError}</p>{/if}
	{#snippet footer()}<Button onclick={() => manualOpen = false}>Cancel</Button><Button variant="primary" onclick={submitManual}>{editingId ? 'Update rule' : 'Add rule'}</Button>{/snippet}
</Dialog>

<Dialog bind:open={pickerOpen} title={core.android ? "Installed applications" : "Running applications"} description={core.android ? "Choose an installed application by package name." : "One entry per executable. Applications without a visible path are matched by name."}>
	{#if core.android}<Field label="Show system apps"><Switch label="Show system apps" bind:checked={showSystem} /></Field>{/if}
	<TextField placeholder={core.android ? "Search by name or package…" : "Search by name or path…"} aria-label="Search running applications" bind:value={query} />
	{#if loadingProcs}<p class="muted">Loading applications…</p>
	{:else if processError}<p class="error" role="alert">{processError}</p><Button onclick={openPicker}>Retry</Button>
	{:else if !filtered.length}<p class="muted">No matching applications.</p>
	{:else}
		<ul class="procs">
			{#each filtered as p (p.packageName ?? p.path ?? `${p.name}:${p.pid}`)}
				<li>{#if p.packageName}<InstalledAppIcon packageName={p.packageName} />{/if}<div class="app-info"><strong>{p.name}</strong><span class="muted mono path" title={p.path ?? ''}>{p.packageName ?? p.path ?? `pid ${p.pid} — matched by name`}</span></div>
					{#if apps.some((a) => sameMatch(a, pickerKind(p), pickerValue(p)))}<Badge tone="success">Added</Badge>
					{:else}<Button size="sm" disabled={saving} onclick={() => add(pickerKind(p), pickerValue(p))}>Add</Button>{/if}
				</li>
			{/each}
		</ul>
	{/if}
</Dialog>

<style>
	.notice { display: flex; gap: var(--space-3); align-items: flex-start; font-size: var(--text-sm); color: var(--color-text-muted); }
	.notice :global(svg) { flex-shrink: 0; color: var(--color-accent); margin-top: 2px; }
	.notice p + p { margin-top: var(--space-2); }
	.toolbar, .mode-notice, .row-actions { display: flex; align-items: center; gap: var(--space-2); flex-wrap: wrap; }
	.add-tools { padding: var(--space-3) var(--space-4); border-bottom: 1px solid var(--color-border); }
	.mode-notice { margin-top: var(--space-3); font-size: var(--text-sm); }
	.error { color: var(--color-danger); font-size: var(--text-sm); white-space: pre-wrap; overflow-wrap: anywhere; }
	.note { font-size: var(--text-xs); margin-top: var(--space-2); }
	.loading { padding: var(--space-4); }
	.apps, .procs { margin: 0; padding: 0; list-style: none; }
	.apps li, .procs li { display: flex; align-items: center; gap: var(--space-3); padding: var(--space-2) var(--space-4); }
	.procs li { padding: var(--space-2) 0; }
	.apps li + li, .procs li + li { border-top: 1px solid var(--color-border); }
	.app-info { display: flex; flex-direction: column; flex: 1; min-width: 0; }
	.path { font-size: var(--text-xs); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
	.kind { font-size: var(--text-xs); }
	.rules { margin: 0; font-family: var(--font-mono); font-size: var(--text-xs); white-space: pre-wrap; word-break: break-all; }
</style>
