const UNITS = ['B', 'KB', 'MB', 'GB', 'TB'];

export function bytes(n: number | null | undefined, digits = 1): string {
	if (n == null || !isFinite(n)) return '—';
	let i = 0;
	while (Math.abs(n) >= 1024 && i < UNITS.length - 1) {
		n /= 1024;
		i++;
	}
	return `${i === 0 ? n.toFixed(0) : n.toFixed(digits)} ${UNITS[i]}`;
}

export const speed = (n: number | null | undefined) => `${bytes(n)}/s`;

export function duration(totalSecs: number): string {
	const s = Math.max(0, Math.floor(totalSecs));
	const d = Math.floor(s / 86400);
	const h = Math.floor((s % 86400) / 3600);
	const m = Math.floor((s % 3600) / 60);
	const sec = s % 60;
	if (d) return `${d}d ${h}h`;
	if (h) return `${h}h ${m}m`;
	if (m) return `${m}m ${sec}s`;
	return `${sec}s`;
}

export function timeAgo(unixSecs: number): string {
	return `${duration(Date.now() / 1000 - unixSecs)} ago`;
}

export function dateTime(unixSecs: number): string {
	return new Date(unixSecs * 1000).toLocaleString();
}

export function clockTime(d: Date | string | number): string {
	return new Date(d).toLocaleTimeString([], { hour12: false });
}

/** 0 means "timeout / failed" in mihomo's delay API. */
export function latencyTone(ms: number | undefined): 'none' | 'good' | 'ok' | 'bad' | 'fail' {
	if (ms === undefined) return 'none';
	if (ms === 0) return 'fail';
	if (ms < 300) return 'good';
	if (ms < 800) return 'ok';
	return 'bad';
}
