<script lang="ts">
	import { untrack } from 'svelte';
	import { Field, Segmented, Select, Switch, TextField } from '#lib/ui/index.ts';
	import { constructRule, parseConstructedRule, ruleTypes, noResolveTypes } from '#lib/rule-constructor.ts';

	let { value = $bindable(''), targets, providers }: { value?: string; targets: string[]; providers: string[] } = $props();
	const initial = untrack(() => parseConstructedRule(value));
	let mode = $state(untrack(() => !value.trim() || initial ? 'builder' : 'inline'));
	let type = $state(initial?.type ?? 'DOMAIN-SUFFIX');
	let payload = $state(initial?.payload ?? '');
	let target = $state(initial?.target ?? 'DIRECT');
	let noResolve = $state(initial?.noResolve ?? false);
	let error = $state('');
	const generated = $derived(constructRule({ type, payload, target, noResolve }));
	const targetOptions = $derived([...new Set([...targets, target].filter(Boolean))].map((value) => ({ value, label: value })));
	const providerOptions = $derived([...new Set([...providers, ...(payload ? [payload] : [])])].map((value) => ({ value, label: providers.includes(value) ? value : `${value} (not in profile)` })));
	const placeholder = $derived(ruleTypes.find((t) => t.value === type)?.placeholder ?? '');
	$effect(() => { if (mode === 'builder') value = generated; });

	function changeMode(next: string) {
		error = '';
		if (next === 'builder' && value.trim()) {
			const parsed = parseConstructedRule(value);
			if (!parsed) { error = 'This rule uses advanced syntax. Continue editing it inline; its text has been preserved.'; return; }
			({ type, payload, target, noResolve } = parsed);
		}
		mode = next;
	}
</script>

<Segmented value={mode} onValueChange={changeMode} label="Rule input method" options={[{ value: 'builder', label: 'Constructor' }, { value: 'inline', label: 'Inline rule' }]} />
{#if mode === 'builder'}
	<Field label="Rule type" stacked><Select bind:value={type} options={ruleTypes} label="Rule type" onValueChange={(next) => { payload = next === 'NETWORK' ? 'tcp' : next === 'RULE-SET' ? providers[0] ?? '' : ''; noResolve = false; }} /></Field>
	{#if type === 'RULE-SET'}
		<Field label="Rule-provider" stacked description="Includes inherited providers and providers added to your current draft.">
			{#if providers.length}<Select bind:value={payload} options={providerOptions} label="Rule-provider" placeholder="Choose a provider" />
			{:else}<TextField aria-label="Rule-provider name" bind:value={payload} placeholder="Provider name" /><p class="hint">No providers found. Add one in Custom rule-providers, or enter its name.</p>{/if}
		</Field>
	{:else if type === 'NETWORK'}
		<Field label="Protocol" stacked><Select bind:value={payload} label="Network protocol" options={[...new Set(['tcp', 'udp', ...(payload ? [payload] : [])])].map((value) => ({ value, label: value }))} placeholder="Choose TCP or UDP" /></Field>
	{:else if type !== 'MATCH'}
		<Field label="Match value" stacked id="constructor-payload"><TextField id="constructor-payload" bind:value={payload} {placeholder} /></Field>
	{:else}<p class="hint">Matches all remaining traffic. Place more specific rules before this rule.</p>{/if}
	<Field label="Routing target" stacked><Select bind:value={target} options={targetOptions} label="Routing target" /></Field>
	{#if noResolveTypes.has(type)}<Field label="Skip DNS resolution" description="Adds no-resolve: match only when an IP is already available."><Switch label="Skip DNS resolution" bind:checked={noResolve} /></Field>{/if}
	{#if payload.includes(',') && type !== 'MATCH'}<p class="error">Use Inline rule for values containing commas, including complex regexes and logical expressions.</p>{/if}
	<Field label="Rule preview" stacked><code class="preview">{generated || 'Choose a match value and routing target.'}</code></Field>
{:else}
	<Field label="Inline rule" stacked id="rule-definition" description="Paste any mihomo rule, including AND/OR/NOT expressions and advanced parameters."><TextField id="rule-definition" bind:value placeholder="DOMAIN-SUFFIX,example.com,DIRECT" /></Field>
{/if}
{#if error}<p class="error" role="alert">{error}</p>{/if}

<style>
	.hint { color: var(--color-text-muted); font-size: var(--text-xs); }
	.error { color: var(--color-danger); font-size: var(--text-xs); }
	.preview { font-size: var(--text-xs); overflow-wrap: anywhere; user-select: text; }
</style>
