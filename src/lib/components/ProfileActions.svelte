<script lang="ts">
    import { FilePlus, FolderOpen, Link } from '@lucide/svelte';
    import { open } from '@tauri-apps/plugin-dialog';
    import { Button, Dialog, Field, TextField, toast } from '#lib/ui/index.ts';
    import YamlEditor from '#lib/components/YamlEditor.svelte';
    import { backend } from '#lib/api/backend.ts';
    import { core } from '#lib/stores/core.svelte.ts';
    import { validProfileInterval } from '#lib/profile-interval.ts';
    import { PROFILE_TEMPLATE } from '#lib/profile-template.ts';
    let { onadded }: { onadded?: () => Promise<void> } = $props();
    let subscription = $state(false);
    let editor = $state(false);
    let busy = $state(false);
    let url = $state('');
    let name = $state('');
    let interval = $state<string | number | undefined>('0');
    let intervalTouched = $state(false);
    let inlineName = $state('New profile');
    let yaml = $state(PROFILE_TEMPLATE);
    const validInterval = $derived(validProfileInterval(interval));
    async function create(action: () => Promise<unknown>) {
        busy = true;
        try {
            await action();
            await core.refreshSettings();
            await onadded?.();
            subscription = editor = false;
            toast.success('Profile added');
        } catch (e) { toast.error(e); }
        finally { busy = false; }
    }
    async function importFile() {
        try {
            const path = await open({ multiple: false, directory: false, filters: [{ name: 'YAML profile', extensions: ['yaml', 'yml'] }, { name: 'All files', extensions: ['*'] }] });
            if (typeof path === 'string') await create(() => backend.profileImportFile(path));
        } catch (e) { toast.error(e); }
    }
</script>
<div class="actions">
    <Button disabled={busy} onclick={() => { inlineName = 'New profile'; yaml = PROFILE_TEMPLATE; editor = true; }}><FilePlus size={14} /> New inline</Button>
    <Button loading={busy} onclick={importFile}><FolderOpen size={14} /> Import file</Button>
    <Button variant="primary" disabled={busy} onclick={() => { url = name = ''; interval = '0'; intervalTouched = false; subscription = true; }}><Link size={14} /> Add subscription</Button>
</div>
<Dialog bind:open={subscription} title="Add subscription" description="The link must return a Clash/mihomo YAML config.">
    <Field label="Subscription URL" stacked id="new-sub-url"><TextField id="new-sub-url" placeholder="https://…" bind:value={url} /></Field>
    <Field label="Name (optional)" stacked id="new-sub-name"><TextField id="new-sub-name" bind:value={name} placeholder="Taken from the server if empty" /></Field>
    <Field label="Update interval (minutes)" description="0 disables automatic refresh." stacked id="new-sub-interval"><TextField id="new-sub-interval" type="number" min="0" max="4294967295" step="1" bind:value={interval} oninput={() => intervalTouched = true} /></Field>
    {#snippet footer()}<Button disabled={busy} onclick={() => subscription = false}>Cancel</Button><Button variant="primary" loading={busy} disabled={!/^https?:\/\//.test(url.trim()) || !validInterval} onclick={() => create(() => backend.profileAddRemote(url.trim(), name || undefined, Number(interval), intervalTouched))}>Download</Button>{/snippet}
</Dialog>
<Dialog bind:open={editor} wide title="New inline profile">
    <TextField bind:value={inlineName} placeholder="Profile name" aria-label="Profile name" />
    <div class="editor"><YamlEditor bind:value={yaml} /></div>
    {#snippet footer()}<Button disabled={busy} onclick={() => editor = false}>Cancel</Button><Button variant="primary" loading={busy} disabled={!inlineName.trim()} onclick={() => create(() => backend.profileCreateInline(inlineName.trim(), yaml))}>Save</Button>{/snippet}
</Dialog>
<style>
    .actions { display: flex; flex-wrap: wrap; gap: var(--space-2); }
    .editor { flex: 1; min-height: 0; }
</style>
