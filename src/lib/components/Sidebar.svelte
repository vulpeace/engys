<script lang="ts">
	import { page } from '$app/state';
	import { Gauge, Globe, AppWindow, FileText, ListTree, ListFilter, Settings } from '@lucide/svelte';
	import { core } from '#lib/stores/core.svelte.ts';

	const items = [
		{ href: '/', label: 'Dashboard', icon: Gauge },
		{ href: '/proxies', label: 'Proxies', icon: Globe },
		{ href: '/apps', label: 'Proxied apps', icon: AppWindow },
		{ href: '/rules', label: 'Rules', icon: ListFilter },
		{ href: '/profiles', label: 'Profiles', icon: FileText },
		{ href: '/requests', label: 'Requests', icon: ListTree },
		{ href: '/settings', label: 'Settings', icon: Settings }
	];

	const isActive = (href: string) =>
		href === '/' ? page.url.pathname === '/' : page.url.pathname.startsWith(href);
</script>

<nav>
	<div class="brand">Mihomo</div>
	<ul>
		{#each items as item (item.href)}
			<li>
				<a href={item.href} class:active={isActive(item.href)} aria-current={isActive(item.href) ? 'page' : undefined}>
					<item.icon size={16} />
					{item.label}
				</a>
			</li>
		{/each}
	</ul>
	<div class="status" class:on={core.running}>
		<span class="dot"></span>
		{core.running ? 'Core running' : 'Core stopped'}
	</div>
</nav>

<style>
	nav {
		display: flex;
		flex-direction: column;
		width: var(--sidebar-width);
		flex-shrink: 0;
		padding: var(--space-4) var(--space-3);
		background: var(--color-surface);
		border-right: 1px solid var(--color-border);
	}
	.brand {
		padding: 0 var(--space-2) var(--space-4);
		font-size: var(--text-lg);
		font-weight: var(--weight-bold);
	}
	ul {
		display: flex;
		flex-direction: column;
		gap: 2px;
		margin: 0;
		padding: 0;
		list-style: none;
	}
	a {
		display: flex;
		align-items: center;
		gap: var(--space-3);
		padding: var(--space-2);
		border-radius: var(--radius-md);
		color: var(--color-text-muted);
		text-decoration: none;
		font-weight: var(--weight-medium);
	}
	a:hover {
		background: var(--color-surface-2);
		color: var(--color-text);
	}
	a.active {
		background: var(--color-accent-soft);
		color: var(--color-accent);
	}
	.status {
		display: flex;
		align-items: center;
		gap: var(--space-2);
		margin-top: auto;
		padding: var(--space-2);
		font-size: var(--text-xs);
		color: var(--color-text-muted);
	}
	.dot {
		width: 8px;
		height: 8px;
		border-radius: var(--radius-full);
		background: var(--color-text-muted);
	}
	.on .dot {
		background: var(--color-success);
	}
</style>
