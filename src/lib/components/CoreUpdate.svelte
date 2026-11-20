<script module lang="ts">
	import { listen } from '@tauri-apps/api/event';
	import { toast } from '#lib/ui/index.ts';
	import { backend, type UpdateInfo } from '#lib/api/backend.ts';
	import { core } from '#lib/stores/core.svelte.ts';

	// Dashboard and Settings share one updater across card mounts.
	let info = $state<UpdateInfo | null>(null);
	let checking = $state(false);
	let installing = $state(false);
	let changingRoute = $state(false);
	let downloadWithoutProxy = $derived(core.settings?.downloadWithoutProxy ?? false);
	type DownloadProgress = { downloaded: number; total: number };
	let progress = $state<DownloadProgress | null>(null);

	async function changeRoute(enabled: boolean) {
		if (changingRoute || installing) return;
		changingRoute = true;
		try {
			await backend.coreSetDownloadWithoutProxy(enabled);
			await core.refreshSettings();
		} catch (e) {
			toast.error(e);
		} finally {
			downloadWithoutProxy = core.settings?.downloadWithoutProxy ?? false;
			changingRoute = false;
		}
	}

	async function check() {
		if (checking || installing || changingRoute) return;
		checking = true;
		try {
			info = await backend.coreCheckUpdate();
		} catch (e) {
			toast.error(e);
		} finally {
			checking = false;
		}
	}

	async function install() {
		if (checking || installing || changingRoute) return;
		installing = true;
		let unlisten: (() => void) | undefined;
		try {
			unlisten = await listen<DownloadProgress>('core-update-progress', ({ payload }) => {
				progress = payload;
			});
			const tag = await backend.coreInstallUpdate();
			toast.success(`Core ${tag} installed`);
		} catch (e) {
			toast.error(e);
		} finally {
			unlisten?.();
			progress = null;
			// Path/version changed; a replaced binary has lost its TUN permission.
			await Promise.all([core.refreshBinary(), core.refreshTunPermission(), core.refreshSettings()])
				.catch((e) => toast.error(e));
			info = await backend.coreCheckUpdate().catch(() => info);
			installing = false;
		}
	}
</script>

<script lang="ts">
	import { Download, RefreshCw } from '@lucide/svelte';
	import { Badge, Button, Meter, Switch } from '#lib/ui/index.ts';
	import { bytes } from '#lib/format.ts';

	let { standalone = false }: { standalone?: boolean } = $props();

	const label = $derived(
		!info ? '' : !info.currentVersion ? `Download ${info.latestVersion}` : info.managed ? `Update to ${info.latestVersion}` : `Download ${info.latestVersion}`
	);
</script>

<div class={['update', { standalone }]}>
	<div class="row">
		<div class="text">
			{#if info}
				{#if info.updateAvailable || !info.managed}
					<span>
						Latest release: <strong>{info.latestVersion}</strong>
						{#if !info.managed && info.currentVersion}
							<span class="muted">— you're using an external core ({info.currentVersion}); downloading switches to the app-managed one.</span>
						{/if}
					</span>
				{:else}
					<span><Badge tone="success">Up to date</Badge> <span class="muted">{info.latestVersion}</span></span>
				{/if}
			{:else}
				<span class="muted">Official builds from github.com/MetaCubeX/mihomo, stored in the app's data folder.</span>
			{/if}
		</div>
		<div class="actions">
			<Button size="sm" loading={checking} disabled={installing || changingRoute} onclick={check}><RefreshCw size={14} /> Check</Button>
			{#if info && (info.updateAvailable || !info.managed)}
				<Button size="sm" variant="primary" loading={installing} disabled={checking || changingRoute} onclick={install}><Download size={14} /> {label}</Button>
			{/if}
		</div>
	</div>
	{#if core.settings}
		<div class="route-option">
			<Switch label="Download without proxy" bind:checked={downloadWithoutProxy}
				onCheckedChange={changeRoute} disabled={installing || checking || changingRoute} />
			<span class="muted small">Download without proxy</span>
		</div>
	{/if}
	{#if installing && progress}
		<Meter value={progress.downloaded} max={progress.total} label="Download progress" />
		<span class="muted small">{bytes(progress.downloaded)} / {bytes(progress.total)}</span>
	{/if}
	{#if info?.updateAvailable && core.tunPermission?.method === 'capabilities'}
		<p class="muted small">Updating replaces the binary, so TUN permission has to be granted again afterwards (one password prompt).</p>
	{/if}
</div>

<style>
	.update {
		display: flex;
		flex-direction: column;
		gap: var(--space-2);
		padding: var(--space-3) 0;
		border-top: 1px solid var(--color-border);
		font-size: var(--text-sm);
	}
	.update.standalone {
		padding: 0;
		border-top: 0;
	}
	.row {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: var(--space-3);
	}
	.standalone .row {
		flex-wrap: wrap;
	}
	.standalone .actions {
		flex-wrap: wrap;
		flex-shrink: 1;
	}
	.actions {
		display: flex;
		gap: var(--space-2);
		flex-shrink: 0;
	}
	.route-option {
		display: flex;
		align-items: center;
		gap: var(--space-2);
	}
	.small {
		font-size: var(--text-xs);
	}
</style>
