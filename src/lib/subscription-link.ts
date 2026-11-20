import { validProfileInterval } from './profile-interval.ts';

export type SubscriptionLink = {
	url: string;
	name?: string;
	refresh?: number;
};

export function parseSubscriptionLink(value: string): SubscriptionLink {
	const link = new URL(value);
	if (link.protocol !== 'engys:' || link.host !== 'import-sub' ||
		link.username || link.password || (link.pathname && link.pathname !== '/')) {
		throw new Error('Unsupported engys link.');
	}

	const urls = link.searchParams.getAll('url');
	const url = urls[0]?.trim();
	if (urls.length !== 1 || !url || !/^https?:\/\//i.test(url)) {
		throw new Error('A subscription link must include one valid HTTP or HTTPS URL.');
	}

	new URL(url);

	const intervals = link.searchParams.getAll('refresh');
	const interval = intervals[0];
	if (intervals.length > 1 || (interval !== undefined &&
		(!/^\d+$/.test(interval) || !validProfileInterval(interval)))) {
		throw new Error('The refresh interval must be a whole number of minutes from 0 to 4294967295.');
	}

	return {
		url,
		name: decodeURIComponent(link.hash.slice(1)).trim() || undefined,
		refresh: interval === undefined ? undefined : Number(interval)
	};
}
