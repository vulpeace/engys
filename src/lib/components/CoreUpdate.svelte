<script lang="ts">
	import { listen } from '@tauri-apps/api/event';
	import { Download, RefreshCw } from '@lucide/svelte';
	import { Badge, Button, Meter, toast } from '#lib/ui/index.ts';
	import { backend, type UpdateInfo } from '#lib/api/backend.ts';
	import { core } from '#lib/stores/core.svelte.ts';
	import { bytes } from '#lib/format.ts';

	let info = $state<UpdateInfo | null>(null);
	let checking = $state(false);
	let installing = $state(false);
	let progress = $state<{ downloaded: number; total: number } | null>(null);

	$effect(() => {
		const un = listen<{ downloaded: number; total: number }>('core-update-progress', (e) => (progress = e.payload));
		return () => {
			un.then((f) => f());
		};
	});

	async function check() {
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
		installing = true;
		progress = null;
		try {
			const tag = await backend.coreInstallUpdate();
			toast.success(`Core ${tag} installed`);
		} catch (e) {
			toast.error(e);
		} finally {
			installing = false;
			progress = null;
			// Path/version changed; a replaced binary has lost its TUN permission.
			await Promise.all([core.refreshBinary(), core.refreshTunPermission(), core.refreshSettings()]);
			info = await backend.coreCheckUpdate().catch(() => info);
		}
	}

	const label = $derived(
		!info ? '' : !info.currentVersion ? `Download ${info.latestVersion}` : info.managed ? `Update to ${info.latestVersion}` : `Download ${info.latestVersion}`
	);
</script>

<div class="update">
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
			<Button size="sm" loading={checking} disabled={installing} onclick={check}><RefreshCw size={14} /> Check</Button>
			{#if info && (info.updateAvailable || !info.managed)}
				<Button size="sm" variant="primary" loading={installing} onclick={install}><Download size={14} /> {label}</Button>
			{/if}
		</div>
	</div>
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
	.row {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: var(--space-3);
	}
	.actions {
		display: flex;
		gap: var(--space-2);
		flex-shrink: 0;
	}
	.small {
		font-size: var(--text-xs);
	}
</style>
