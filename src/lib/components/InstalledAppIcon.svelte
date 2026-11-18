<script lang="ts">
    import { onMount } from 'svelte';
    import { AppWindow } from '@lucide/svelte';
    import { installedAppIcon } from '#lib/stores/installed-apps.ts';
    let { packageName }: { packageName: string } = $props();
    let element: HTMLSpanElement;
    let src = $state<string | null>(null);
    onMount(() => {
        let active = true;
        const observer = new IntersectionObserver((entries) => {
            if (!entries.some((entry) => entry.isIntersecting)) return;
            observer.disconnect();
            void installedAppIcon(packageName).then((icon) => { if (active) src = icon; });
        });
        observer.observe(element);
        return () => { active = false; observer.disconnect(); };
    });
</script>
<span bind:this={element}>{#if src}<img {src} width="32" height="32" alt="" />{:else}<AppWindow size={24} />{/if}</span>
<style>span { display: inline-flex; width: 32px; height: 32px; align-items: center; justify-content: center; flex-shrink: 0; }</style>
