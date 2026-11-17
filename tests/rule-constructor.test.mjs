import { test } from 'node:test';
import assert from 'node:assert/strict';
import { constructRule, parseConstructedRule, ruleTypes } from '../src/lib/rule-constructor.ts';

test('constructor and parser round-trip every offered rule type', () => {
	for (const { value: type, placeholder: payload } of ruleTypes) {
		const fields = { type, payload, target: 'My proxy group', noResolve: false };
		assert.deepEqual(parseConstructedRule(constructRule(fields)), fields, type);
	}
});
test('catch-all has no match value, and no-resolve only applies to eligible types', () => {
	assert.equal(constructRule({ type: 'MATCH', payload: 'ignored', target: 'DIRECT', noResolve: true }), 'MATCH,DIRECT');
	assert.equal(constructRule({ type: 'RULE-SET', payload: 'My provider', target: 'My group', noResolve: true }), 'RULE-SET,My provider,My group,no-resolve');
	assert.equal(constructRule({ type: 'DOMAIN', payload: 'example.com', target: 'REJECT', noResolve: true }), 'DOMAIN,example.com,REJECT');
	assert.equal(parseConstructedRule('IP-CIDR,192.0.2.0/24,DIRECT,no-resolve').noResolve, true);
});
test('unsupported syntax stays inline instead of losing expressions or parameters', () => {
	for (const rule of [
		'AND,((NETWORK,TCP),(DST-PORT,443)),DIRECT',
		'OR,((DOMAIN,a.test),(DOMAIN,b.test)),DIRECT',
		'NOT,((DOMAIN,a.test)),REJECT',
		'IP-CIDR,192.0.2.0/24,DIRECT,src',
		'RULE-SET,MyProvider,DIRECT,no-resolve,src',
		'DOMAIN-REGEX,^[a-z]{1,3}\\.example$,DIRECT',
		'MATCH,DIRECT,extra',
		'DOMAIN,example.com,DIRECT,no-resolve',
		'DOMAIN,example.com,DIRECT\nMATCH,REJECT'
	]) assert.equal(parseConstructedRule(rule), null, rule);
});
test('incomplete or ambiguous constructor values cannot enter the draft', () => {
	for (const fields of [
		{ type: 'DOMAIN', payload: '', target: 'DIRECT' },
		{ type: 'DOMAIN', payload: 'example.com', target: '' },
		{ type: 'DOMAIN', payload: 'a,b', target: 'DIRECT' },
		{ type: 'DOMAIN', payload: 'example.com', target: 'a,b' },
		{ type: 'DOMAIN', payload: 'a\nb', target: 'DIRECT' },
		{ type: 'UNKNOWN', payload: 'example.com', target: 'DIRECT' }
	]) assert.equal(constructRule({ ...fields, noResolve: false }), '');
});
test('process paths and regex backslashes remain intact', () => {
	for (const rule of ['PROCESS-PATH,C:\\Program Files\\App\\app.exe,Work', 'PROCESS-NAME-REGEX,(?i)^firefox\\.exe$,DIRECT']) {
		assert.equal(constructRule(parseConstructedRule(rule)), rule);
	}
});
