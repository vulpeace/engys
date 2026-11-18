import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import ts from 'typescript';
const source = ts.transpileModule(readFileSync(new URL('../src/lib/profile-interval.ts', import.meta.url), 'utf8'), {
    compilerOptions: { target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.ESNext }
}).outputText;
const { validProfileInterval } = await import(`data:text/javascript;base64,${Buffer.from(source).toString('base64')}`);
test('clearing a numeric interval disables submission without throwing', () => {
    for (const value of [undefined, null, '', '   ']) assert.equal(validProfileInterval(value), false);
});
test('editing from zero through empty to a new interval stays valid', () => {
    assert.deepEqual([0, undefined, 3, 30, '30'].map(validProfileInterval), [true, false, true, true, true]);
});
test('accepts zero and whole minutes within the backend limit', () => {
    for (const value of [0, '0', 1, '120', 4294967295]) assert.equal(validProfileInterval(value), true);
    for (const value of [-1, 1.5, 4294967296, NaN, Infinity, 'invalid']) assert.equal(validProfileInterval(value), false);
});
