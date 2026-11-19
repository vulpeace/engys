<script lang="ts">
	import type { SourceRuleChoice } from '#lib/api/backend.ts';
	import { Button, Dialog } from '#lib/ui/index.ts';

	let { name, busy = false, error = '', onchoice, ondiscard }: {
		name: string | undefined;
		busy?: boolean;
		error?: string;
		onchoice: (choice: SourceRuleChoice | null) => void;
		ondiscard?: () => void;
	} = $props();
</script>

<Dialog bind:open={() => name !== undefined, (open) => { if (!open && !busy) onchoice(null); }}
	title={ondiscard ? 'Subscription update ready for review' : 'The source rules changed'}
	description={`Choose which rules to use for ${name ?? 'this profile'}.`}>
	<div>
		<Button variant="primary" disabled={busy} onclick={() => onchoice('merge')}>Merge with subscription</Button>
		<p>Use the new source rules and insert your added and edited rules at their previous positions.</p>
	</div>
	<div>
		<Button disabled={busy} onclick={() => onchoice('override')}>Override subscription</Button>
		<p>Keep the profile’s entire current rule list in its current order.</p>
	</div>
	<div>
		<Button disabled={busy} onclick={() => onchoice('subscription')}>Use subscription rules</Button>
		<p>Use the new source rule list and clear local changes to the list.</p>
	</div>
	{#if error}<p class="error selectable" role="alert">{error}</p>{/if}
	{#snippet footer()}
		{#if ondiscard}<Button disabled={busy} onclick={ondiscard}>Discard update</Button>{/if}
		<Button disabled={busy} onclick={() => onchoice(null)}>{ondiscard ? 'Review later' : 'Cancel'}</Button>
	{/snippet}
</Dialog>

<style>
	p { margin: var(--space-1) 0 0; color: var(--color-text-muted); font-size: var(--text-xs); }
	.error { color: var(--color-danger); overflow-wrap: anywhere; }
</style>
