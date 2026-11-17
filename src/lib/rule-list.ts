import { parse } from 'yaml';
import type { RoutingEntry } from './api/backend.ts';

/** Accept plain mihomo lines or a copied YAML rules list without splitting rule commas. */
export function parsePastedRules(text: string): string[] {
	const trimmed = text.trim();
	if (!trimmed) return [];
	const meaningful = trimmed.split(/\r?\n/).map((line) => line.trim()).filter((line) => line && !line.startsWith('#'));
	if (!meaningful.length) return [];
	let rules: unknown = meaningful;
	if (/^(?:-\s|rules\s*:)/.test(meaningful[0])) {
		const yaml = parse(trimmed);
		rules = Array.isArray(yaml) ? yaml : yaml?.rules;
	}
	if (!Array.isArray(rules) || rules.some((rule) => typeof rule !== 'string' || !rule.trim() || /[\r\n]/.test(rule))) {
		throw new Error('Paste one rule per line, or a YAML list of rule strings.');
	}
	return rules.map((rule: string) => rule.trim());
}

/** Positions always refer to the complete list, including hidden search results and other pages. */
export function moveRule(entries: RoutingEntry[], from: number, to: number, allowInherited: boolean): RoutingEntry[] {
	if (!Number.isInteger(from) || !Number.isInteger(to) || from < 0 || to < 0 || from >= entries.length || to >= entries.length || from === to) return entries;
	if (entries[from].sourceIndex !== null && !allowInherited) return entries;
	const next = [...entries];
	const [entry] = next.splice(from, 1);
	next.splice(to, 0, entry);
	return next;
}
