<script lang="ts">
	import '#lib/ui/base.css';
	import { untrack } from 'svelte';
	import type { LayoutProps } from './$types';
	import Sidebar from '#lib/components/Sidebar.svelte';
	import SubscriptionLinkImport from '#lib/components/SubscriptionLinkImport.svelte';
	import PendingSubscriptionReviews from '#lib/components/PendingSubscriptionReviews.svelte';
	import { Toaster, EmptyState } from '#lib/ui/index.ts';
	import { core } from '#lib/stores/core.svelte.ts';
	import { live } from '#lib/stores/live.svelte.ts';

	let { children }: LayoutProps = $props();

	let ready = $state(false);
	let initError = $state<string | null>(null);

	core
		.init()
		.then(() => (ready = true))
		.catch((e) => (initError = String(e)));

	// Theme: 'system' leaves it to prefers-color-scheme (see tokens.css).
	$effect(() => {
		const theme = core.settings?.theme ?? 'system';
		if (theme === 'system') delete document.documentElement.dataset.theme;
		else document.documentElement.dataset.theme = theme;
	});

	$effect(() => {
		live.historyLimit = core.settings?.requestHistoryLimit ?? 2000;
	});

	const runningSession = $derived(core.status?.running ? `${core.status.pid}:${core.status.startedAt}` : null);
	const controller = $derived(core.status?.controller);
	const secret = $derived(core.status?.secret);
	let session: string | null = null;

	// Reset counters on stop or a new process, while preserving them on config reload.
	$effect(() => {
		const nextSession = runningSession;
		// Status polling replaces the object even when the stream endpoint is unchanged.
		const api = nextSession && controller !== undefined && secret !== undefined ? untrack(() => core.api) : null;
		untrack(() => {
			live.disconnect();
			if (!nextSession) live.clearConnections();
			if (nextSession !== session || !api) live.resetSession();
			session = nextSession;
			if (api) live.connect(api);
		});
	});
</script>

<div class="app" class:android={core.android}>
	<Sidebar />
	<main>
		{#if initError}
			<EmptyState
				title="Backend unavailable"
				description="This UI must run inside the Tauri app (pnpm tauri dev). {initError}"
			/>
		{:else if ready}
			{@render children()}
		{/if}
	</main>
</div>
{#if ready}
	<SubscriptionLinkImport />
	<PendingSubscriptionReviews />
{/if}
<Toaster />

<style>
	.app {
		display: flex;
		height: 100dvh;
		overflow: hidden;
	}
	main {
		flex: 1;
		min-width: 0;
		display: flex;
		flex-direction: column;
		gap: var(--space-4);
		padding: var(--space-5);
		overflow-y: auto;
	}
    @media (max-width: 700px) {
        .app { flex-direction: column; padding-top: env(safe-area-inset-top); padding-bottom: env(safe-area-inset-bottom); }
        main { padding: var(--space-3); }
    }
    .app.android { flex-direction: column; padding-top: env(safe-area-inset-top); padding-bottom: env(safe-area-inset-bottom); }
</style>
