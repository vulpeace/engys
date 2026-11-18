import type { Settings } from '#lib/api/backend.ts';

export const PROFILE_FIELDS = {
	'external-controller': ['controllerPort'], secret: ['secret'], 'mixed-port': ['mixedPort'],
	'allow-lan': ['allowLan'], ipv6: ['ipv6'], 'log-level': ['logLevel'], mode: ['mode'],
	'tun.include-package': ['tun', 'includePackage'], 'tun.exclude-package': ['tun', 'excludePackage'],
	'tun.enable': ['tun', 'enable'], 'tun.stack': ['tun', 'stack'],
	'tun.auto-route': ['tun', 'autoRoute'], 'tun.strict-route': ['tun', 'strictRoute'],
	'tun.auto-detect-interface': ['tun', 'autoDetectInterface'], 'tun.dns-hijack': ['tun', 'dnsHijack'],
	'tun.dns-mode': ['tun', 'dnsMode'], 'tun.device': ['tun', 'device'], 'tun.mtu': ['tun', 'mtu'],
	'dns.nameserver': ['tun', 'dnsNameservers']
} as const;
export type ProfileField = keyof typeof PROFILE_FIELDS;
export const PROFILE_FIELD_LABELS: Record<ProfileField, string> = {
	'external-controller': 'controller port', secret: 'controller secret', 'mixed-port': 'mixed port',
	'allow-lan': 'Allow LAN', ipv6: 'IPv6', 'log-level': 'log level', mode: 'mode',
	'tun.include-package': 'included Android apps', 'tun.exclude-package': 'excluded Android apps',
	'tun.enable': 'TUN mode', 'tun.stack': 'TUN stack', 'tun.auto-route': 'auto route',
	'tun.strict-route': 'strict route', 'tun.auto-detect-interface': 'interface detection',
	'tun.dns-hijack': 'DNS hijack', 'tun.dns-mode': 'system DNS integration',
	'tun.device': 'device name', 'tun.mtu': 'MTU', 'dns.nameserver': 'fallback DNS servers'
};

export function overrideKeys(settings: Settings, field: ProfileField, enabled = true): string[] {
	const keys = new Set(settings.overrides);
	if (enabled) keys.add(field); else keys.delete(field);
	return [...keys].sort();
}

function copyValue(to: Settings, from: Settings, field: ProfileField) {
	const path = PROFILE_FIELDS[field];
	let target = to as unknown as Record<string, unknown>;
	let source = from as unknown as Record<string, unknown>;
	for (const part of path.slice(0, -1)) {
		target = target[part] as Record<string, unknown>;
		source = source[part] as Record<string, unknown>;
	}
	const key = path[path.length - 1];
	target[key] = structuredClone(source[key]);
}

export function resetField(settings: Settings, inherited: Settings, field: ProfileField): Settings {
	const next = structuredClone(settings);
	copyValue(next, inherited, field);
	next.overrides = overrideKeys(next, field, false);
	return next;
}

export function syncInherited(settings: Settings, inherited: Settings): Settings {
	const next = structuredClone(settings);
	for (const field of Object.keys(PROFILE_FIELDS) as ProfileField[]) {
		if (!next.overrides.includes(field)) copyValue(next, inherited, field);
	}
	return next;
}

// Inherited values may change while a form is open; only local preferences and
// explicitly overridden values should make that form dirty.
export function comparableSettings(settings: Settings | null) {
	if (!settings) return null;
	const value = structuredClone(settings);
	const defaults = structuredClone(settings);
	const target = defaults as unknown as Record<string, unknown>;
	for (const field of Object.keys(PROFILE_FIELDS) as ProfileField[]) {
		if (field === 'tun.enable' || !value.overrides.includes(field)) {
			const path = PROFILE_FIELDS[field];
			let object = target;
			for (const part of path.slice(0, -1)) object = object[part] as Record<string, unknown>;
			object[path[path.length - 1]] = null;
		}
	}
	defaults.overrides = value.overrides.filter((key) => key !== 'tun.enable').sort();
	return defaults;
}
