import { test, beforeEach } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { compileModule } from 'svelte/compiler';
import ts from 'typescript';

// Exercise the actual rune-backed store with controllable API streams.
const source = ts.transpileModule(readFileSync(new URL('../src/lib/stores/live.svelte.ts', import.meta.url), 'utf8'), {
	compilerOptions: { target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.ESNext }
}).outputText;
const compiled = compileModule(source, { filename: 'live.svelte.js', generate: 'client' }).js.code
	.replace(/from '([^']+)'/g, (_, specifier) => `from '${import.meta.resolve(specifier)}'`);
const { live } = await import(`data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`);

function connect() {
	const callbacks = {};
	live.connect({ stream(path, callback) { callbacks[path] = callback; return () => {}; } });
	return {
		traffic: (upTotal, downTotal) => callbacks['/traffic']({ up: 0, down: 0, upTotal, downTotal }),
		connections: (uploadTotal, downloadTotal, connections = []) => callbacks['/connections?interval=1000']({ uploadTotal, downloadTotal, connections })
	};
}

beforeEach(() => { live.disconnect(); live.resetSession(); });

test('new process totals replace the previous session and include bytes before the first frame', () => {
	let api = connect();
	api.traffic(1000, 2000);
	live.disconnect();
	live.resetSession();
	assert.equal(live.uploadTotal, 0);
	assert.equal(live.samples.t.length, 0);
	api = connect();
	api.traffic(12, 34);
	assert.equal(live.uploadTotal, 12);
	assert.equal(live.downloadTotal, 34);
});

test('connection totals keep updating when traffic frames omit totals', () => {
	const api = connect();
	api.traffic(undefined, undefined);
	api.connections(10, 20);
	api.connections(30, 50);
	assert.equal(live.uploadTotal, 30);
	assert.equal(live.downloadTotal, 50);
});

test('traffic totals take precedence over delayed connection snapshots, including zero', () => {
	const api = connect();
	api.traffic(0, 40);
	api.connections(100, 100);
	assert.equal(live.uploadTotal, 0);
	assert.equal(live.downloadTotal, 40);
});

test('reconnecting within the same process preserves its counters and chart', () => {
	const first = connect();
	first.traffic(100, 200);
	const next = connect();
	assert.equal(live.uploadTotal, 100);
	assert.equal(live.samples.t.length, 1);
	next.traffic(120, 240);
	assert.equal(live.uploadTotal, 120);
	assert.equal(live.samples.t.length, 2);
});

test('a fresh snapshot reopens connections that survived a stream reconnect', () => {
	const first = connect();
	const open = { id: 'open', upload: 1, download: 2 };
	const closed = { id: 'closed', upload: 3, download: 4 };
	first.connections(4, 6, [open, closed]);

	const next = connect();
	next.connections(5, 7, [{ ...open, upload: 2, download: 3 }]);

	assert.equal(live.activeCount, 1);
	assert.equal(live.records.find(({ conn }) => conn.id === open.id).closedAt, null);
	assert.notEqual(live.records.find(({ conn }) => conn.id === closed.id).closedAt, null);
});

test('disconnect publishes closed records while preserving the previous snapshot', () => {
	const api = connect();
	const conn = { id: 'disconnect-test', upload: 1, download: 2 };
	api.connections(1, 2, [conn]);
	api.connections(3, 6, [{ ...conn, upload: 3, download: 6 }]);
	const previous = live.records.find((record) => record.conn.id === conn.id);
	assert.equal(previous.closedAt, null);
	assert.equal(previous.downSpeed, 4);

	live.disconnect();

	const closed = live.records.find((record) => record.conn.id === conn.id);
	assert.equal(live.activeCount, 0);
	assert.ok(closed.closedAt > 0);
	assert.equal(closed.upSpeed, 0);
	assert.equal(closed.downSpeed, 0);
	assert.equal(previous.closedAt, null);
});

test('callbacks from a stopped session cannot restore old totals after restart', () => {
	const previous = connect();
	previous.traffic(1000, 2000);
	live.disconnect();
	live.resetSession();
	const current = connect();
	current.traffic(10, 20);
	previous.traffic(1000, 2000);
	previous.connections(1000, 2000);
	assert.equal(live.uploadTotal, 10);
	assert.equal(live.downloadTotal, 20);
	assert.equal(live.samples.t.length, 1);
});

test('clearing connections after stop removes both active and closed history', () => {
	const api = connect();
	const active = { id: 'active-at-stop', upload: 0, download: 0 };
	const closed = { id: 'closed-before-stop', upload: 0, download: 0 };
	api.connections(0, 0, [active, closed]);
	api.connections(0, 0, [active]);

	live.disconnect();
	live.clearConnections();
	assert.deepEqual(live.records, []);
	assert.equal(live.activeCount, 0);

	api.connections(0, 0, [active]);
	connect().connections(0, 0);
	assert.deepEqual(live.records, []);
});
