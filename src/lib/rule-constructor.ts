// Keep advanced/ambiguous syntax in the inline editor rather than rewriting it.
export const ruleTypes = [
	['DOMAIN-SUFFIX', 'Domain suffix', 'example.com'], ['DOMAIN', 'Exact domain', 'www.example.com'],
	['DOMAIN-KEYWORD', 'Domain keyword', 'example'], ['DOMAIN-WILDCARD', 'Domain wildcard', '*.example.com'],
	['DOMAIN-REGEX', 'Domain regex', '^.*\\.example\\.com$'], ['GEOSITE', 'Geosite category', 'google'],
	['IP-CIDR', 'Destination IPv4 CIDR', '192.0.2.0/24'], ['IP-CIDR6', 'Destination IPv6 CIDR', '2001:db8::/32'],
	['GEOIP', 'Destination GeoIP', 'US'], ['IP-ASN', 'Destination ASN', '13335'],
	['IP-SUFFIX', 'Destination IP suffix', '0.0.8.8/16'], ['SRC-IP-CIDR', 'Source IP CIDR', '192.168.1.0/24'],
	['SRC-GEOIP', 'Source GeoIP', 'US'], ['SRC-IP-ASN', 'Source ASN', '13335'], ['SRC-IP-SUFFIX', 'Source IP suffix', '0.0.8.8/16'],
	['DST-PORT', 'Destination port', '443'], ['SRC-PORT', 'Source port', '1024-65535'], ['IN-PORT', 'Inbound port', '7890'],
	['PROCESS-NAME', 'Process name', 'firefox.exe'], ['PROCESS-PATH', 'Process path', '/usr/bin/firefox'],
	['PROCESS-NAME-REGEX', 'Process name regex', '(?i)^firefox'], ['PROCESS-PATH-REGEX', 'Process path regex', '^/opt/apps/.*'],
	['PROCESS-NAME-WILDCARD', 'Process name wildcard', 'firefox*'], ['PROCESS-PATH-WILDCARD', 'Process path wildcard', '/opt/apps/*'],
	['NETWORK', 'Network protocol', 'tcp'], ['UID', 'User ID', '1000'], ['DSCP', 'DSCP', '4'],
	['IN-TYPE', 'Inbound type', 'SOCKS'], ['IN-NAME', 'Inbound name', 'my-inbound'], ['IN-USER', 'Inbound user', 'username'],
	['RULE-SET', 'Rule-provider', 'MyProvider'], ['MATCH', 'Match all (catch-all)', '']
].map(([value, label, placeholder]) => ({ value, label: `${label} · ${value}`, placeholder }));
export const noResolveTypes = new Set(['IP-CIDR', 'IP-CIDR6', 'GEOIP', 'IP-ASN', 'IP-SUFFIX', 'RULE-SET']);
export type RuleFields = { type: string; payload: string; target: string; noResolve: boolean };
export function constructRule(fields: RuleFields): string {
	const { type, noResolve } = fields;
	const payload = fields.payload.trim(), target = fields.target.trim();
	if (!ruleTypes.some((t) => t.value === type) || !target || /[,\r\n]/.test(target)) return '';
	if (type !== 'MATCH' && (!payload || /[,\r\n]/.test(payload))) return '';
	return [type, ...(type === 'MATCH' ? [] : [payload]), target,
		...(noResolve && noResolveTypes.has(type) ? ['no-resolve'] : [])].join(',');
}
export function parseConstructedRule(rule: string): RuleFields | null {
	if (/[\r\n]/.test(rule)) return null;
	const parts = rule.trim().split(',').map((p) => p.trim());
	const type = parts[0];
	if (!ruleTypes.some((t) => t.value === type)) return null;
	if (type === 'MATCH') return parts.length === 2 && !!parts[1]
		? { type, payload: '', target: parts[1], noResolve: false } : null;
	const noResolve = parts.length === 4 && parts[3] === 'no-resolve' && noResolveTypes.has(type);
	if ((parts.length !== 3 && !noResolve) || !parts[1] || !parts[2]) return null;
	return { type, payload: parts[1], target: parts[2], noResolve };
}
