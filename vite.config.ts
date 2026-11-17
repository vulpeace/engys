import adapter from '@sveltejs/adapter-static';
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

// Tauri expects a fixed dev port and must not have the screen cleared under it.
export default defineConfig({
	clearScreen: false,
	server: {
		port: 5173,
		strictPort: true,
		watch: { ignored: ['**/src-tauri/**'] }
	},
	plugins: [
		sveltekit({
			compilerOptions: {
				runes: ({ filename }) =>
					filename.split(/[/\\]/).includes('node_modules') ? undefined : true
			},
			// SPA: everything is rendered client-side inside the Tauri webview.
			adapter: adapter({ fallback: 'index.html' })
		})
	]
});
