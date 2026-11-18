import assert from 'node:assert/strict';
import { test } from 'node:test';
import { parseSubscriptionLink } from '../src/lib/subscription-link.ts';

const base = 'mihomo-client://import-sub?';
const url = 'https://example.com/config.yaml?key=a%2Bb&format=mihomo#section';
const query = `url=${encodeURIComponent(url)}`;

test('preserves the full nested URL and decodes the profile name once', () => {
	assert.deepEqual(parseSubscriptionLink(`${base}${query}&refresh=60#%E2%98%83%20A%2BB%2520`), {
		url, refresh: 60, name: '☃ A+B%20'
	});
});

test('distinguishes an omitted refresh interval from explicit zero', () => {
	assert.equal(parseSubscriptionLink(`${base}${query}`).refresh, undefined);
	assert.equal(parseSubscriptionLink(`${base}${query}&refresh=0`).refresh, 0);
	assert.equal(parseSubscriptionLink(`${base}${query}`).name, undefined);
});

test('rejects malformed import links and unsupported subscription schemes', () => {
	for (const link of [
		base, `${base}url=file%3A%2F%2F%2Fconfig.yaml`, `${base}url=https%3A%2F%2F`,
		`${base}${query}&${query}`, `${base}${query}#%FF`,
		`mihomo-client://unknown?${query}`
	]) assert.throws(() => parseSubscriptionLink(link));

	for (const refresh of ['', '-1', '1.5', '1e2', '4294967296', '0&refresh=1']) {
		assert.throws(() => parseSubscriptionLink(`${base}${query}&refresh=${refresh}`));
	}
});
