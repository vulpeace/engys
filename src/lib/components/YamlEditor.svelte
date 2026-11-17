<script lang="ts">
	import { onMount } from 'svelte';
	import { basicSetup } from 'codemirror';
	import { EditorView } from '@codemirror/view';
	import { EditorState } from '@codemirror/state';
	import { yaml } from '@codemirror/lang-yaml';

	let { value = $bindable('') }: { value?: string } = $props();

	let el: HTMLDivElement;
	let view: EditorView | null = null;

	// Editor chrome follows the design tokens, so it themes with the app.
	const theme = EditorView.theme({
		'&': { height: '100%', fontSize: 'var(--text-sm)', backgroundColor: 'var(--color-surface)', color: 'var(--color-text)' },
		'.cm-scroller': { fontFamily: 'var(--font-mono)' },
		'.cm-gutters': { backgroundColor: 'var(--color-surface-2)', color: 'var(--color-text-muted)', border: 'none' },
		'.cm-activeLine, .cm-activeLineGutter': { backgroundColor: 'color-mix(in srgb, var(--color-accent) 8%, transparent)' },
		'&.cm-focused': { outline: 'none' },
		'.cm-cursor': { borderLeftColor: 'var(--color-text)' },
		'&.cm-focused .cm-selectionBackground, .cm-selectionBackground': { backgroundColor: 'var(--color-accent-soft) !important' }
	});

	onMount(() => {
		view = new EditorView({
			parent: el,
			state: EditorState.create({
				doc: value,
				extensions: [
					basicSetup,
					yaml(),
					theme,
					EditorView.updateListener.of((u) => {
						if (u.docChanged) value = u.state.doc.toString();
					})
				]
			})
		});
		return () => view?.destroy();
	});

	// External changes (e.g. content loaded after mount).
	$effect(() => {
		const v = value;
		if (view && v !== view.state.doc.toString()) {
			view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: v } });
		}
	});
</script>

<div class="editor" bind:this={el}></div>

<style>
	.editor {
		height: 100%;
		min-height: 240px;
		border: 1px solid var(--color-border);
		border-radius: var(--radius-md);
		overflow: hidden;
	}
	.editor :global(.cm-editor) {
		height: 100%;
	}
</style>
