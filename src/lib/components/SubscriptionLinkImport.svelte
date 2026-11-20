<script lang="ts">
	import { onMount } from 'svelte';
	import { invoke } from '@tauri-apps/api/core';
	import { listen } from '@tauri-apps/api/event';
	import { backend } from '#lib/api/backend.ts';
	import { parseSubscriptionLink, type SubscriptionLink } from '#lib/subscription-link.ts';
	import { Button, Dialog, toast } from '#lib/ui/index.ts';

	let current = $state.raw<{ raw: string; link: SubscriptionLink }>();
	let busy = $state(false);

	onMount(() => {
		let disposed = false;
		let stop: (() => void) | undefined;
		let collecting = false;
		let collectAgain = false;

		async function collect() {
			collectAgain = true;
			if (collecting) return;
			collecting = true;
			try {
				while (collectAgain && !disposed) {
					collectAgain = false;
					const raw = await invoke<string | null>('pending_subscription_link');
					if (disposed) return;
					if (!raw) {
						current = undefined;
					} else if (current?.raw !== raw) {
						try {
							current = { raw, link: parseSubscriptionLink(raw) };
						} catch (error) {
							toast.error(error instanceof URIError || error instanceof TypeError
								? 'Invalid subscription link.'
								: error);
							await invoke('dismiss_subscription_link', { link: raw });
							collectAgain = true;
						}
					}
				}
			} catch (error) {
				if (!disposed) toast.error(error);
			} finally {
				collecting = false;
			}
		}

		void listen('subscription-links-pending', collect).then((unlisten) => {
			if (disposed) unlisten();
			else {
				stop = unlisten;
				void collect();
			}
		}).catch(toast.error);

		return () => {
			disposed = true;
			stop?.();
		};
	});

	async function dismiss(open: boolean) {
		if (open || busy || !current) return;
		busy = true;
		try {
			await invoke('dismiss_subscription_link', { link: current.raw });
		} catch (error) {
			toast.error(error);
		} finally {
			busy = false;
		}
	}

	async function download() {
		if (busy || !current) return;
		const { raw, link } = current;
		busy = true;
		try {
			await backend.profileAddRemote(link.url, link.name, link.refresh, link.refresh !== undefined, raw);
			toast.success('Profile added');
		} catch (error) {
			toast.error(error);
		} finally {
			busy = false;
		}
	}
</script>

<Dialog bind:open={() => current !== undefined, dismiss} title="Import subscription" description="Review the subscription before downloading it.">
	{#if current}
		<dl>
			<dt>Subscription URL</dt>
			<dd class="selectable">{current.link.url}</dd>
			{#if current.link.name}
				<dt>Name</dt>
				<dd>{current.link.name}</dd>
			{/if}
			<dt>Update interval</dt>
			<dd>{current.link.refresh === undefined ? 'Default' : current.link.refresh === 0 ? 'Disabled' : `${current.link.refresh} min`}</dd>
		</dl>
	{/if}
	{#snippet footer()}
		<Button disabled={busy} onclick={() => dismiss(false)}>Cancel</Button>
		<Button variant="primary" loading={busy} onclick={download}>Download</Button>
	{/snippet}
</Dialog>

<style>
	dl { margin: 0; }
	dt { font-weight: var(--weight-bold); }
	dd { margin: 0 0 var(--space-3); overflow-wrap: anywhere; }
	dd:last-child { margin-bottom: 0; }
</style>
