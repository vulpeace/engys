<script lang="ts">
	import { onMount } from 'svelte';
	import { listen } from '@tauri-apps/api/event';
	import { backend, type PendingSubscriptionReview, type SourceRuleChoice } from '#lib/api/backend.ts';
	import SourceRuleChoiceDialog from './SourceRuleChoiceDialog.svelte';
	import { toast } from '#lib/ui/index.ts';

	let reviews = $state.raw<PendingSubscriptionReview[]>([]);
	let deferred = $state.raw(new Set<string>());
	let busy = $state(false);
	let error = $state('');
	let request = 0;
	const review = $derived(reviews.find((item) => !deferred.has(item.token)));

	async function refresh(showDeferred = false) {
		if (document.hidden) return;
		if (showDeferred) deferred = new Set();
		const current = ++request;
		try {
			const pending = await backend.pendingSubscriptionReviews();
			if (current !== request || document.hidden) return;
			reviews = pending;
		} catch (e) { toast.error(e); }
	}

	onMount(() => {
		const reopen = () => { void refresh(true); };
		const listeners = [
			listen('profiles-changed', () => { void refresh(); }),
			listen('main-window-shown', reopen)
		];
		for (const listener of listeners) void listener.catch(toast.error);
		window.addEventListener('focus', reopen);
		document.addEventListener('visibilitychange', reopen);
		reopen();
		return () => {
			++request;
			window.removeEventListener('focus', reopen);
			document.removeEventListener('visibilitychange', reopen);
			for (const listener of listeners) void listener.then((stop) => stop(), () => {});
		};
	});

	async function choose(choice: SourceRuleChoice | null) {
		if (!review || busy) return;
		error = '';
		if (!choice) {
			deferred = new Set([...deferred, review.token]);
			return;
		}
		await resolve(choice);
	}

	async function resolve(choice: SourceRuleChoice | null) {
		if (!review || busy) return;
		const { profileId, profileName, token } = review;
		error = '';
		busy = true;
		try {
			await backend.resolvePendingSubscription(profileId, token, choice);
			reviews = reviews.filter((item) => item.token !== token);
			toast.success(choice ? `Updated ${profileName}` : 'Subscription update discarded');
		} catch (e) {
			error = String(e);
		} finally {
			busy = false;
			void refresh();
		}
	}
</script>

<SourceRuleChoiceDialog name={review?.profileName} {busy} {error} onchoice={choose} ondiscard={() => resolve(null)} />
