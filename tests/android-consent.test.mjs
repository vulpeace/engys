import { test, beforeEach } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { compileModule } from 'svelte/compiler';
import ts from 'typescript';

const calls = [];
let granted = false;
let approve = true;
globalThis.__androidConsentBackend = {
    tunPermission: async () => { calls.push('check'); return { granted }; },
    tunGrantPermission: async () => { calls.push('request'); granted = approve; return { granted }; },
    coreStart: async () => { calls.push('start'); return { running: true, controller: 'http://localhost', secret: '' }; },
    androidVpnActive: async () => true,
};
let source = ts.transpileModule(readFileSync(new URL('../src/lib/stores/core.svelte.ts', import.meta.url), 'utf8'), {
    compilerOptions: { target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.ESNext }
}).outputText;
source = source.replace(/import .*?from '@tauri-apps\/api\/event';/, 'const listen = async () => () => {};')
    .replace(/import .*?from '#lib\/settings-overrides.ts';/, 'const overrideKeys = () => [];')
    .replace(/import .*?from '#lib\/api\/backend.ts';/, 'const backend = globalThis.__androidConsentBackend;')
    .replace(/import .*?from '#lib\/api\/mihomo.ts';/, 'class Mihomo {}');
const compiled = compileModule(source, { filename: 'core.svelte.js', generate: 'client' }).js.code
    .replace(/from '([^']+)'/g, (_, specifier) => `from '${import.meta.resolve(specifier)}'`);
const { core } = await import(`data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`);
beforeEach(() => {
    calls.length = 0; granted = false; approve = true;
    core.platform = 'android'; core.status = null;
    core.settings = { tun: { enable: true }, overrides: [] };
});
test('Android asks for consent before starting the core', async () => {
    await core.start();
    assert.deepEqual(calls, ['check', 'request', 'start']);
    assert.equal(core.busy, false);
});
test('denying consent prevents startup and clears busy state', async () => {
    approve = false;
    await assert.rejects(core.start(), /VPN permission was not granted/);
    assert.deepEqual(calls, ['check', 'request']);
    assert.equal(core.busy, false);
});
test('an existing grant starts without another prompt', async () => {
    granted = true;
    await core.start();
    assert.deepEqual(calls, ['check', 'start']);
});
test('proxy-only startup needs no VPN consent', async () => {
    core.settings.tun.enable = false;
    await core.start();
    assert.deepEqual(calls, ['start']);
});
