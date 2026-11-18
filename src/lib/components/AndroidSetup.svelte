<script lang="ts">
    import InstalledAppIcon from '#lib/components/InstalledAppIcon.svelte';
    import { installedApps } from '#lib/stores/installed-apps.ts';
    import { onMount } from 'svelte';
    import { backend, type AndroidSetupStatus, type ProcessEntry, type Settings } from '#lib/api/backend.ts';
    import { overrideKeys, resetField } from '#lib/settings-overrides.ts';
    import { Badge, Button, Card, Dialog, Field, Switch, TextField, toast } from '#lib/ui/index.ts';
    let { settings = $bindable() }: { settings: Settings } = $props();
    let status = $state<AndroidSetupStatus | null>(null);
    let picker = $state(false);
    let loading = $state(false);
    let apps = $state<ProcessEntry[]>([]);
    let query = $state('');
    let showSystem = $state(false);
    let list = $state<'includePackage' | 'excludePackage'>('includePackage');
    const filtered = $derived(apps.filter((app) => (showSystem || !app.system) && `${app.name} ${app.packageName}`.toLowerCase().includes(query.toLowerCase())));
    async function refresh() { status = await backend.androidSetupStatus(); }
    onMount(() => {
        void refresh().catch(toast.error);
        const resume = () => { if (document.visibilityState === 'visible') void refresh().catch(toast.error); };
        document.addEventListener('visibilitychange', resume);
        return () => document.removeEventListener('visibilitychange', resume);
    });
    async function request(kind: 'battery' | 'notifications') {
        try {
            if (kind === 'battery') await backend.androidRequestBattery();
            else await backend.androidRequestNotifications();
            await refresh();
        } catch (e) { toast.error(e); }
    }
    function update(field: 'includePackage' | 'excludePackage', values: string[]) {
        settings.tun[field] = [...new Set(values.map((v) => v.trim()).filter(Boolean))];
        settings.overrides = overrideKeys(settings, field === 'includePackage' ? 'tun.include-package' : 'tun.exclude-package');
    }
    async function inherit(field: 'includePackage' | 'excludePackage') {
        try {
            const inherited = await backend.settingsInherited();
            settings = resetField(settings, inherited, field === 'includePackage' ? 'tun.include-package' : 'tun.exclude-package');
        } catch (e) { toast.error(e); }
    }
    async function pick(field: typeof list) {
        list = field; picker = true; loading = true;
        try { apps = await installedApps(); } catch (e) { toast.error(e); }
        finally { loading = false; }
    }
</script>
<Card title="Android background operation">
    <Field label="Battery optimization" description="Allow unrestricted background operation for a more reliable VPN. Some devices also have a separate vendor battery setting.">
        <Badge tone={status?.batteryExempt ? 'success' : 'neutral'}>{status?.batteryExempt ? 'Exempt' : 'Optimized'}</Badge>
        <Button onclick={() => request('battery')}>Battery settings</Button>
    </Field>
    <Field label="Connection notification" description="A silent ongoing notification keeps the VPN service in the foreground. Closing the app leaves the service running.">
        <Badge tone={status?.notifications ? 'success' : 'neutral'}>{status?.notifications ? 'Allowed' : 'Disabled'}</Badge>
        <Button onclick={() => request('notifications')}>Allow notifications</Button>
    </Field>
    <Button size="sm" onclick={() => refresh().catch(toast.error)}>Refresh status</Button>
</Card>
<Card title="VPN application filtering">
    <p class="muted">Included apps enter the VPN; an empty include list captures all apps. Exclusions are subtracted from that selection. These settings follow the profile until edited. Save to reconnect with the new selection.</p>
    {#each ['includePackage', 'excludePackage'] as field}
        {@const key = field as 'includePackage' | 'excludePackage'}
        <Field label={key === 'includePackage' ? 'Included packages' : 'Excluded packages'} stacked>
            <TextField value={settings.tun[key].join(', ')} onchange={(event) => update(key, event.currentTarget.value.split(','))} placeholder="com.example.app, …" />
            <Button onclick={() => pick(key)}>Choose installed apps</Button>
            {#if settings.overrides.includes(key === 'includePackage' ? 'tun.include-package' : 'tun.exclude-package')}
                <Button onclick={() => inherit(key)}>Use profile default</Button>
            {/if}
        </Field>
    {/each}
</Card>
<Dialog bind:open={picker} title={list === 'includePackage' ? 'Include applications' : 'Exclude applications'} description="Selections apply when you save Settings.">
    <TextField bind:value={query} placeholder="Search by name or package…" aria-label="Search installed applications" />
    <Field label="Show system apps"><Switch label="Show system apps" bind:checked={showSystem} /></Field>
    {#if loading}<p>Loading applications…</p>{:else}
        <ul>
            {#each filtered as app (app.packageName)}
                {@const name = app.packageName!}
                <li>
                    {#if app.packageName}<InstalledAppIcon packageName={app.packageName} />{/if}
                    <span>{app.name}<small>{name}</small></span>
                    <Switch label={`Select ${app.name}`} checked={settings.tun[list].includes(name)} onCheckedChange={(checked) => update(list, checked ? [...settings.tun[list], name] : settings.tun[list].filter((v) => v !== name))} />
                </li>
            {/each}
        </ul>
    {/if}
</Dialog>
<style>
    p { font-size: var(--text-sm); }
    ul { list-style: none; margin: 0; padding: 0; }
    li { display: flex; align-items: center; gap: 12px; padding: 8px 0; }
    li span { flex: 1; min-width: 0; overflow-wrap: anywhere; }
    small { display: block; color: var(--color-text-muted); }
</style>
