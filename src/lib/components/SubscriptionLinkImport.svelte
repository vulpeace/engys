<script lang="ts">
	import { onMount } from 'svelte';
	import { invoke } from '@tauri-apps/api/core';
	import { listen } from '@tauri-apps/api/event';
	import { backend } from '#lib/api/backend.ts';
	import { parseSubscriptionLink, type SubscriptionLink } from '#lib/subscription-link.ts';
	import { Button, Dialog, toast } from '#lib/ui/index.ts';

	let pending = $state<SubscriptionLink[]>([]);
	let current = $state<SubscriptionLink>();
	let open = $state(false);
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
					const links = await invoke<string[]>('take_subscription_links');
					if (disposed) return;
					for (const link of links) {
						try {
							pending.push(parseSubscriptionLink(link));
						} catch (error) {
							toast.error(error instanceof URIError || error instanceof TypeError
								? 'Invalid subscription link.'
								: error);
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

	$effect(() => {
		if (!open && !busy && pending.length) {
			current = pending.shift();
			open = current !== undefined;
		}
	});

	async function download(link: SubscriptionLink) {
		busy = true;
		try {
			await backend.profileAddRemote(link.url, link.name, link.refresh, link.refresh !== undefined);
			open = false;
			toast.success('Profile added');
		} catch (error) {
			toast.error(error);
		} finally {
			busy = false;
		}
	}
</script>

<Dialog bind:open title="Import subscription" description="Review the subscription before downloading it.">
	{#if current}
		<dl>
			<dt>Subscription URL</dt>
			<dd class="selectable">{current.url}</dd>
			{#if current.name}
				<dt>Name</dt>
				<dd>{current.name}</dd>
			{/if}
			<dt>Update interval</dt>
			<dd>{current.refresh === undefined ? 'Default' : current.refresh === 0 ? 'Disabled' : `${current.refresh} min`}</dd>
		</dl>
	{/if}
	{#snippet footer()}
		<Button disabled={busy} onclick={() => open = false}>Cancel</Button>
		<Button variant="primary" loading={busy} onclick={() => current && download(current)}>Download</Button>
	{/snippet}
</Dialog>

<style>
	dl { margin: 0; }
	dt { font-weight: var(--weight-bold); }
	dd { margin: 0 0 var(--space-3); overflow-wrap: anywhere; }
	dd:last-child { margin-bottom: 0; }
</style>
