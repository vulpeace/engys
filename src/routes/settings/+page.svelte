<script lang="ts">
	import { goto } from '$app/navigation';
	import { onMount } from 'svelte';
	import { listen } from '@tauri-apps/api/event';
	import { comparableSettings, PROFILE_FIELD_LABELS, overrideKeys, resetField, syncInherited, type ProfileField } from '#lib/settings-overrides.ts';
	import { AppWindow, ListFilter, ListTree, FolderOpen, RefreshCw, Save, Undo2 } from '@lucide/svelte';
	import { open as openFile } from '@tauri-apps/plugin-dialog';
	import { Button, Card, Field, PageHeader, Segmented, Select, Switch, TextField, toast } from '#lib/ui/index.ts';
	import { backend, type Settings, type LoginStatus } from '#lib/api/backend.ts';
	import AndroidSetup from '#lib/components/AndroidSetup.svelte';
	import CoreUpdate from '#lib/components/CoreUpdate.svelte';
	import { core } from '#lib/stores/core.svelte.ts';

	// Edit a draft; nothing is persisted until Save.
	let draft = $state<Settings>(structuredClone($state.snapshot(core.settings!)));
	let saving = $state(false);
	let login = $state<LoginStatus | null>(null);
	let loginBusy = $state(false);
	let loginChecked = $state(false);
	let loginError = $state('');

	async function refreshLogin() {
		loginBusy = true;
		try {
			login = await backend.loginStatus();
			loginChecked = login.enabled;
			loginError = '';
		} catch (e) {
			loginError = String(e);
		} finally {
			loginChecked = login?.enabled ?? false;
			loginBusy = false;
		}
	}
	if (!core.android) void refreshLogin();

	async function setLogin(enabled: boolean) {
		loginBusy = true;
		try {
			login = await backend.loginSet(enabled);
			loginChecked = login.enabled;
			loginError = '';
			toast.success(enabled ? 'Start at login enabled' : 'Start at login disabled');
		} catch (e) {
			toast.error(e);
			await refreshLogin();
		} finally {
			loginChecked = login?.enabled ?? false;
			loginBusy = false;
		}
	}
	const binary = $derived(core.binary);
	let workDir = $state('');

	let inherited = $state<Settings | null>(null);
	function mark(field: ProfileField) { draft.overrides = overrideKeys($state.snapshot(draft), field); }
	function reset(field: ProfileField) {
		if (inherited) draft = resetField($state.snapshot(draft), $state.snapshot(inherited), field);
	}
	async function refreshInherited() {
		const value = await backend.settingsInherited();
		inherited = value;
		draft = syncInherited($state.snapshot(draft), value);
	}
	onMount(() => {
		void refreshInherited().catch(toast.error);
		const unlisten = listen('profiles-changed', () => { void refreshInherited().catch(toast.error); }).catch(toast.error);
		return () => { void unlisten.then((stop) => stop?.()); };
	});
	const comparable = comparableSettings;
	const dirty = $derived(JSON.stringify(comparable($state.snapshot(draft))) !== JSON.stringify(comparable($state.snapshot(core.settings))));
	const list = (v: string) => v.split(',').map((x) => x.trim()).filter(Boolean);
	const needsRestart = $derived(
		core.running &&
			(draft.controllerPort !== core.settings?.controllerPort ||
				draft.secret !== core.settings?.secret ||
				(draft.corePath ?? '') !== (core.settings?.corePath ?? ''))
	);

	async function detect() {
		await Promise.all([core.refreshBinary(), core.refreshTunPermission()]);
	}
	backend.workDir().then((d) => (workDir = d));

	async function save() {
		saving = true;
		try {
			const restart = needsRestart;
			const pathChanged = (draft.corePath ?? '') !== (core.settings?.corePath ?? '');
			// TUN on/off is owned by the Dashboard toggle; don't overwrite it from a stale draft.
			const snap = $state.snapshot(draft);
			const flags = new Set(snap.overrides);
			flags.delete('tun.enable');
			if (core.settings!.overrides.includes('tun.enable')) flags.add('tun.enable');
			const next = { ...snap, overrides: [...flags].sort(), tun: { ...snap.tun, enable: core.settings!.tun.enable } };
			const applyCore = JSON.stringify({ ...next, minimizeToTray: core.settings!.minimizeToTray }) !== JSON.stringify(core.settings);
			await core.saveSettings(next);
			draft = structuredClone($state.snapshot(core.settings!));
			if (pathChanged) detect();
			if (restart) {
				toast.info('Saved. Restart the core to apply controller or binary changes.');
			} else if (core.running && applyCore) {
				await backend.coreReload();
				toast.success('Saved and applied to the running core');
			} else {
				toast.success('Settings saved');
			}
		} catch (e) {
			toast.error(e);
		} finally {
			saving = false;
		}
	}

	async function browseCore() {
		const path = await openFile({ multiple: false, directory: false, title: 'Choose the mihomo binary' });
		if (typeof path === 'string') draft.corePath = path;
	}

	// Theme is a pure UI preference: apply it immediately.
	async function setTheme(theme: string) {
		draft.theme = theme as Settings['theme'];
		await core.saveSettings({ theme: draft.theme }).catch(toast.error);
	}

	const port = (v: unknown) => Math.min(65535, Math.max(1, Number(v) || 0));
</script>

{#snippet resetControl(field: ProfileField)}
	{#if inherited && draft.overrides.includes(field)}
		<Button icon size="sm" disabled={saving} title={`Reset ${PROFILE_FIELD_LABELS[field]} to subscription`} aria-label={`Reset ${PROFILE_FIELD_LABELS[field]} to subscription`} onclick={() => reset(field)}><Undo2 size={14} /></Button>
	{/if}
{/snippet}

<PageHeader title="Settings">
	{#snippet actions()}
		<Button disabled={!dirty} onclick={() => (draft = structuredClone($state.snapshot(core.settings!)))}>
			<Undo2 size={14} /> Revert
		</Button>
		<Button variant="primary" disabled={!dirty} loading={saving} onclick={save}><Save size={14} /> Save</Button>
	{/snippet}
</PageHeader>

{#if core.android}
<Card title="Tools">
    <div class="android-tools">
        <Button onclick={() => goto('/apps')}><AppWindow size={16} /> Proxied apps</Button>
        <Button onclick={() => goto('/rules')}><ListFilter size={16} /> Rules</Button>
        <Button onclick={() => goto('/requests')}><ListTree size={16} /> Requests</Button>
    </div>
</Card>
{/if}

<p class="muted note">Profile-backed fields follow the active subscription or config until you edit them. The reset button returns control to the profile; click Save to apply.</p>

{#if core.android}<AndroidSetup bind:settings={draft} />{:else}
<Card title="Startup & tray">
	<Field label="Start at login" description={login?.detail ?? 'Launch in the background and restore the previous core state.'}>
		{#if login}
			<Switch label="Start at login" bind:checked={loginChecked}
				disabled={loginBusy || !login.supported} onCheckedChange={setLogin} />
		{:else if loginBusy}
			<span class="muted small" role="status">Checking…</span>
		{/if}
		<Button icon title="Refresh login status" disabled={loginBusy} onclick={refreshLogin}><RefreshCw size={14} /></Button>
		{#if login?.needsRepair}<Button disabled={loginBusy} onclick={() => setLogin(true)}>Repair</Button>{/if}
	</Field>
	{#if loginError}<p class="err" role="alert">{loginError}</p>{/if}
	<p class="muted small">Start at login applies immediately. Use the tray icon or open the app again to show the background window.</p>
	<Field label="Minimize to tray" description="Hide the window when minimized or closed, keeping the core running. Choose Quit in the tray to exit. Click Save to apply.">
		<Switch label="Minimize to tray" bind:checked={draft.minimizeToTray} />
	</Field>
</Card>

{/if}

<Card title="Appearance">
	<Field label="Theme">
		<Segmented
			label="Theme"
			value={draft.theme}
			onValueChange={setTheme}
			options={[
				{ value: 'system', label: 'System' },
				{ value: 'light', label: 'Light' },
				{ value: 'dark', label: 'Dark' }
			]}
		/>
	</Field>
</Card>

<Card title="Core">
	{#if !core.android}<Field label="mihomo binary" description="Leave empty to use the app-managed core (download below), else a bundled one, else `mihomo` from PATH." stacked id="core-path">
		<TextField id="core-path" placeholder="Auto-detect" bind:value={draft.corePath} />
		<Button icon title="Browse" onclick={browseCore}><FolderOpen size={14} /></Button>
		<Button icon title="Detect" onclick={detect}><RefreshCw size={14} /></Button>
	</Field>
	{/if}
	<div class="detected muted mono selectable">
		{#if !binary}Detecting…
		{:else if binary.error}<span class="err">{binary.error}</span>
		{:else}{binary.version}<br />{binary.path}{/if}
	</div>
	{#if core.android}<p class="muted note">The Android core is bundled. Update the application to update mihomo.</p>{:else}<CoreUpdate />{/if}
	<Field label="Working directory" description="Runtime config, GEO databases and cache live here.">
		<span class="mono muted small selectable">{workDir}</span>
	</Field>
	<Field label="Log level">
		<Select
			label="Log level"
			bind:value={draft.logLevel} onValueChange={() => mark('log-level')}
			options={['silent', 'error', 'warning', 'info', 'debug'].map((v) => ({ value: v, label: v }))}
		/>
			{@render resetControl('log-level')}
	</Field>
	<Field label="Default mode" description="Also changeable live on the Proxies tab.">
		<Select
			label="Default mode"
			bind:value={draft.mode} onValueChange={() => mark('mode')}
			options={[
				{ value: 'rule', label: 'Rule' },
				{ value: 'global', label: 'Global' },
				{ value: 'direct', label: 'Direct' }
			]}
		/>
			{@render resetControl('mode')}
	</Field>
</Card>

<Card title="Network">
	<Field
		label="Mixed port"
		description="HTTP + SOCKS5 proxy port for apps and the system proxy. Leave empty to use the profile's own setting."
		id="mixed-port"
	>
		<TextField
			id="mixed-port"
			type="number"
			class="narrow"
			placeholder="From profile"
			value={draft.mixedPort ?? ''}
			oninput={(e) => { draft.mixedPort = e.currentTarget.value.trim() ? port(e.currentTarget.value) : null; draft.overrides = overrideKeys($state.snapshot(draft), 'mixed-port', draft.mixedPort !== null); }}
		/>
			{@render resetControl('mixed-port')}
	</Field>
	<Field label="Allow LAN" description="Let other devices on your network use this proxy.">
		<Switch label="Allow LAN" bind:checked={draft.allowLan} onCheckedChange={() => mark('allow-lan')} />
			{@render resetControl('allow-lan')}
	</Field>
	<Field label="IPv6">
		<Switch label="IPv6" bind:checked={draft.ipv6} onCheckedChange={() => mark('ipv6')} />
			{@render resetControl('ipv6')}
	</Field>
</Card>

<Card title="TUN">
	<p class="muted note">Turn TUN on or off on the Dashboard. These options apply the next time TUN starts (or immediately if it's running).</p>
	<Field label="Stack" description="mixed: system TCP + gVisor UDP. mips: mihomo's own stack. Try another if something misbehaves.">
		<Select
			label="TUN stack"
			bind:value={draft.tun.stack} onValueChange={() => mark('tun.stack')}
			options={[
				{ value: 'mixed', label: 'Mixed' },
				{ value: 'system', label: 'System' },
				{ value: 'gvisor', label: 'gVisor' },
				{ value: 'mips', label: 'mips' }
			]}
		/>
			{@render resetControl('tun.stack')}
	</Field>
	{#if !core.android}<Field label="Auto route" description="Install routes so all traffic enters the TUN interface.">
		<Switch label="Auto route" bind:checked={draft.tun.autoRoute} onCheckedChange={() => mark('tun.auto-route')} />
			{@render resetControl('tun.auto-route')}
	</Field>
	<Field label="Strict route" description="Prevent leaks (Linux: unsupported networks unreachable; Windows: blocks DNS leaks). May break some apps like VirtualBox.">
		<Switch label="Strict route" bind:checked={draft.tun.strictRoute} onCheckedChange={() => mark('tun.strict-route')} />
			{@render resetControl('tun.strict-route')}
	</Field>
	<Field label="Auto-detect interface" description="Bind outgoing connections to the physical interface to avoid loops.">
		<Switch label="Auto-detect interface" bind:checked={draft.tun.autoDetectInterface} onCheckedChange={() => mark('tun.auto-detect-interface')} />
			{@render resetControl('tun.auto-detect-interface')}
	</Field>
	<Field label="DNS hijack" description="Comma-separated. Matching DNS traffic is answered by the core." id="dns-hijack">
		<TextField id="dns-hijack" class="wide" value={draft.tun.dnsHijack.join(', ')} oninput={(e) => { draft.tun.dnsHijack = list(e.currentTarget.value); mark('tun.dns-hijack'); }} />
			{@render resetControl('tun.dns-hijack')}
	</Field>
	<Field
		label="System DNS integration"
		description="Native/Hijack point the OS resolver at the TUN (on Linux via resolvectl, which asks for your password on every start). Off is enough on Linux: DNS traffic is routed into the TUN and answered by DNS hijack anyway."
	>
		<Select
			label="System DNS integration"
			bind:value={draft.tun.dnsMode} onValueChange={() => mark('tun.dns-mode')}
			options={[
				{ value: 'disabled', label: 'Off' },
				{ value: 'native', label: 'Native' },
				{ value: 'hijack', label: 'Hijack' },
				{ value: '', label: 'Core default' }
			]}
		/>
			{@render resetControl('tun.dns-mode')}
	</Field>
	<Field label="Device name" description="Empty = core default." id="tun-device">
		<TextField id="tun-device" class="narrow" placeholder="Meta" bind:value={draft.tun.device} oninput={() => mark('tun.device')} />
			{@render resetControl('tun.device')}
	</Field>
	{/if}
	<Field label="MTU" description="Empty = core default." id="tun-mtu">
		<TextField id="tun-mtu" type="number" class="narrow" placeholder="default" value={draft.tun.mtu ?? ''} oninput={(e) => { draft.tun.mtu = e.currentTarget.value ? Math.max(576, Number(e.currentTarget.value)) : null; mark('tun.mtu'); }} />
			{@render resetControl('tun.mtu')}
	</Field>
	<Field
		label="Fallback DNS servers"
		description="Used only when the profile has no enabled `dns:` section — TUN needs the core's DNS. Comma-separated."
		stacked
		id="tun-dns"
	>
		<TextField id="tun-dns" value={draft.tun.dnsNameservers.join(', ')} oninput={(e) => { draft.tun.dnsNameservers = list(e.currentTarget.value); mark('dns.nameserver'); }} />
			{@render resetControl('dns.nameserver')}
	</Field>
</Card>

<Card title="Controller API">
	<Field label="Port" description={core.android ? "Preferred controller port. If occupied, Android tries the next available port. The API stays on 127.0.0.1." : "Uses the profile controller port unless overridden. The API stays on 127.0.0.1."} id="ctl-port">
		<TextField id="ctl-port" type="number" class="narrow" value={draft.controllerPort} oninput={(e) => { draft.controllerPort = port(e.currentTarget.value); mark('external-controller'); }} />
			{@render resetControl('external-controller')}
	</Field>
	{#if core.android && core.running}<p class="muted small">Active controller: <span class="mono">{core.status?.controller}</span></p>{/if}
	<Field label="Secret" description="Uses the profile secret unless overridden, with a generated local secret as the fallback." id="secret">
		<TextField id="secret" class="mono" bind:value={draft.secret} oninput={() => mark('secret')} />
		<Button onclick={() => { draft.secret = crypto.randomUUID().replaceAll('-', ''); mark('secret'); }}>Regenerate</Button>
			{@render resetControl('secret')}
	</Field>
</Card>

<Card title="Latency test">
	<Field label="Test URL" stacked id="test-url">
		<TextField id="test-url" bind:value={draft.delayTestUrl} />
	</Field>
	<Field label="Timeout (ms)" id="test-timeout">
		<TextField id="test-timeout" type="number" class="narrow" value={draft.delayTestTimeout} oninput={(e) => (draft.delayTestTimeout = Math.max(500, Number(e.currentTarget.value) || 5000))} />
	</Field>
</Card>

<Card title="Requests & profiles">
	<Field label="Request history limit" description="Closed connections kept on the Requests tab." id="hist">
		<TextField id="hist" type="number" class="narrow" value={draft.requestHistoryLimit} oninput={(e) => (draft.requestHistoryLimit = Math.max(100, Number(e.currentTarget.value) || 2000))} />
	</Field>
	<Field label="Subscription User-Agent" description="Some providers return a different format depending on this." id="ua">
		<TextField id="ua" bind:value={draft.subscriptionUserAgent} />
	</Field>
</Card>

<style>
	.detected {
		margin: 0 0 var(--space-2);
		font-size: var(--text-xs);
	}
	.err {
		color: var(--color-danger);
	}
	.small {
		font-size: var(--text-xs);
	}
	.note {
		font-size: var(--text-xs);
		margin-bottom: var(--space-2);
	}
	:global(.wide) {
		width: 260px !important;
	}
	:global(.narrow) {
		width: 110px !important;
	}
    .android-tools { display: flex; flex-wrap: wrap; gap: var(--space-2); }
</style>
