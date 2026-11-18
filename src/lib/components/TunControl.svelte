<script lang="ts">
	import { goto } from '$app/navigation';
	import { ShieldCheck, ShieldAlert, Undo2 } from '@lucide/svelte';
	import { Badge, Button, Card, Switch, toast } from '#lib/ui/index.ts';
	import { core, versionAtLeast } from '#lib/stores/core.svelte.ts';

	const enabled = $derived(core.settings?.tun.enable ?? false);
	const perm = $derived(core.tunPermission);
	const needsGrant = $derived(perm && !perm.granted && perm.canGrant);
	const unsupported = $derived(perm?.method === 'unsupported');
	// Cores before v1.19.33 ignore `tun.dns-mode` and always call resolvectl,
	// which on Linux means three polkit password prompts per TUN start.
	const promptsForDns = $derived(perm?.method === 'capabilities' && !versionAtLeast(core.version, [1, 19, 33]));

	const state = $derived.by(() => {
		if (!enabled) return { tone: 'neutral', label: 'Off' } as const;
		if (!core.running) return { tone: 'accent', label: 'Starts with core' } as const;
		return core.tunActive
			? ({ tone: 'success', label: 'Active' } as const)
			: ({ tone: 'danger', label: 'Not active' } as const);
	});

	async function toggle(on: boolean) {
		try {
			await core.setTun(on);
			if (on && core.running) toast.success('TUN mode is active');
		} catch (e) {
			toast.error(e);
		}
	}

	async function grant() {
		try {
			await core.grantTunPermission();
			if (core.tunPermission?.granted) {
				toast.success('Permission granted — you can enable TUN now');
			}
		} catch (e) {
			toast.error(e);
		}
	}
</script>

<Card title="TUN mode">
	{#snippet actions()}
		<Badge tone={state.tone}>{state.label}</Badge>
		{#if core.settings?.overrides.includes('tun.enable')}<Button icon size="sm" disabled={core.tunBusy} title="Reset TUN mode to subscription" onclick={() => core.resetTun().catch(toast.error)}><Undo2 size={14} /></Button>{/if}
		<Switch
			label="TUN mode"
			checked={enabled}
			disabled={core.tunBusy || unsupported || (!core.android && !enabled && !perm?.granted)}
			onCheckedChange={toggle}
		/>
	{/snippet}
	<p class="muted text">
		Routes all system traffic through the core via a virtual network interface, including apps that ignore proxy
		settings.
	</p>
	{#if promptsForDns}
		<div class="perm">
			<ShieldAlert size={16} />
			<span class="detail">
				This core ({core.binary?.version?.match(/v[\d.]+/)?.[0]}) asks for your password to configure system DNS every time
				TUN starts. Update it in Settings → Core (v1.19.33+) to avoid that.
			</span>
			<Button size="sm" onclick={() => goto('/settings')}>Open settings</Button>
		</div>
	{/if}
	{#if perm}
		<div class="perm" class:ok={perm.granted}>
			{#if perm.granted}<ShieldCheck size={16} />{:else}<ShieldAlert size={16} />{/if}
			<span class="detail selectable">{perm.detail}</span>
			{#if needsGrant}
				<Button size="sm" variant="primary" loading={core.tunBusy} onclick={grant}>
					{perm.method === 'administrator' ? 'Restart as administrator' : 'Grant permission'}
				</Button>
			{/if}
		</div>
	{/if}
</Card>

<style>
	.text {
		font-size: var(--text-sm);
	}
	.perm {
		display: flex;
		align-items: center;
		gap: var(--space-2);
		margin-top: var(--space-3);
		padding: var(--space-2) var(--space-3);
		border-radius: var(--radius-md);
		background: var(--color-surface-2);
		font-size: var(--text-xs);
		color: var(--color-warning);
	}
	.perm.ok {
		color: var(--color-success);
	}
	.detail {
		flex: 1;
		color: var(--color-text-muted);
		word-break: break-all;
	}
</style>
