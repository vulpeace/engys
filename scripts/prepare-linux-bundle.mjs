import { mkdir, readFile, writeFile } from 'node:fs/promises';

const root = new URL('../', import.meta.url);
const { version } = JSON.parse(await readFile(new URL('package.json', root), 'utf8'));
if (typeof version !== 'string' || !/^\d+\.\d+\.\d+(?:-[\da-zA-Z.-]+)?(?:\+[\da-zA-Z.-]+)?$/.test(version)) {
	throw new Error('package.json must contain a valid app version');
}

// Tauri also uses the Debian desktop template when staging an AppImage.
// Properties dialogs read this key rather than the version in the filename.
const directory = new URL('src-tauri/target/packaging/', root);
await mkdir(directory, { recursive: true });
await writeFile(new URL('engys.desktop', directory), `[Desktop Entry]
Categories={{categories}}
{{#if comment}}
Comment={{comment}}
{{/if}}
Exec={{exec}} %u
StartupWMClass={{exec}}
Icon={{icon}}
Name={{name}}
Terminal=false
Type=Application
{{#if mime_type}}
MimeType={{mime_type}}
{{/if}}
X-AppImage-Version=${version}
`);
console.log(`Prepared Linux desktop metadata for version ${version}`);
