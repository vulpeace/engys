import { test } from 'node:test';
import assert from 'node:assert/strict';
import { parsePastedRules, moveRule } from '../src/lib/rule-list.ts';

test('plain pasted rules preserve order, duplicates, commas, and advanced syntax', () => {
	const rules = ['DOMAIN,a.test,DIRECT', 'AND,((NETWORK,TCP),(DST-PORT,443)),REJECT', 'DOMAIN,a.test,DIRECT'];
	assert.deepEqual(parsePastedRules(`# comment\r\n\n ${rules.join('\r\n')} \n`), rules);
	assert.deepEqual(parsePastedRules('  \n# only a comment\n'), []);
});
test('copied YAML lists and rules mappings decode quoted values and comments', () => {
	const expected = ['DOMAIN,a.test,DIRECT', 'MATCH,REJECT'];
	assert.deepEqual(parsePastedRules('- DOMAIN,a.test,DIRECT\n- "MATCH,REJECT" # comment'), expected);
	assert.deepEqual(parsePastedRules('rules:\n  - DOMAIN,a.test,DIRECT\n  - MATCH,REJECT'), expected);
});
test('malformed YAML and multiline or non-string entries are rejected', () => {
	for (const value of ['- DOMAIN,a.test,DIRECT\n- 42', '- |\n  DOMAIN,a.test,DIRECT\n  MATCH,REJECT', 'rules: {bad: value}', '- "unclosed']) {
		assert.throws(() => parsePastedRules(value));
	}
});
const entries = [
	{ id: 'a', sourceIndex: 0, rule: 'DOMAIN,a.test,DIRECT' },
	{ id: 'custom', sourceIndex: null, rule: 'DOMAIN,custom.test,REJECT' },
	{ id: 'b', sourceIndex: 1, rule: 'MATCH,DIRECT' }
];
test('moving custom rules across inherited rules preserves identity and original array', () => {
	assert.deepEqual(moveRule(entries, 1, 0, false).map((e) => e.id), ['custom', 'a', 'b']);
	assert.deepEqual(moveRule(entries, 1, 2, false).map((e) => e.id), ['a', 'b', 'custom']);
	assert.deepEqual(entries.map((e) => e.id), ['a', 'custom', 'b']);
});
test('inherited movement requires unlocking and all movement respects full-list bounds', () => {
	assert.equal(moveRule(entries, 0, 2, false), entries);
	assert.deepEqual(moveRule(entries, 0, 2, true).map((e) => e.id), ['custom', 'b', 'a']);
	for (const [from, to] of [[0, -1], [2, 3], [-1, 0], [3, 0], [0.5, 1], [1, 1]]) assert.equal(moveRule(entries, from, to, true), entries);
});
