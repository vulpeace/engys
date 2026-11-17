// Client for mihomo's external-controller REST + WebSocket API.
// See Meta-Docs/docs/api/index.en.md.

export type DelayHistory = { time: string; delay: number };

export type Proxy = {
	name: string;
	type: string;
	udp: boolean;
	alive?: boolean;
	history: DelayHistory[];
	'provider-name'?: string;
	// Groups only:
	now?: string;
	all?: string[];
	testUrl?: string;
	hidden?: boolean;
	icon?: string;
	fixed?: string;
};

export type ProxyProvider = {
	name: string;
	type: string;
	vehicleType: 'HTTP' | 'File' | 'Inline' | 'Compatible' | string;
	updatedAt?: string;
	proxies: Proxy[];
	subscriptionInfo?: { Upload: number; Download: number; Total: number; Expire: number };
};

export type ConnectionMetadata = {
	network: string;
	type: string;
	sourceIP: string;
	destinationIP: string;
	sourcePort: string;
	destinationPort: string;
	host: string;
	sniffHost?: string;
	process?: string;
	processPath?: string;
	inboundName?: string;
	dnsMode?: string;
	remoteDestination?: string;
};

export type Connection = {
	id: string;
	metadata: ConnectionMetadata;
	upload: number;
	download: number;
	start: string;
	chains: string[];
	rule: string;
	rulePayload: string;
};

export type ConnectionsSnapshot = {
	downloadTotal: number;
	uploadTotal: number;
	memory: number;
	connections: Connection[] | null;
};

export type Traffic = { up: number; down: number; upTotal?: number; downTotal?: number };
export type Memory = { inuse: number; oslimit: number };
export type LogLine = { type: string; payload: string };

export class ApiError extends Error {
	constructor(
		public status: number,
		message: string
	) {
		super(message);
	}
}

export class Mihomo {
	constructor(
		readonly base: string,
		readonly secret: string
	) {}

	async request<T = void>(method: string, path: string, body?: unknown, timeoutMs = 15000): Promise<T> {
		const res = await fetch(this.base + path, {
			method,
			headers: {
				Authorization: `Bearer ${this.secret}`,
				...(body !== undefined ? { 'Content-Type': 'application/json' } : {})
			},
			body: body !== undefined ? JSON.stringify(body) : undefined,
			signal: AbortSignal.timeout(timeoutMs)
		});
		if (!res.ok) {
			let msg = res.statusText;
			try {
				msg = (await res.json()).message ?? msg;
			} catch {
				/* not JSON */
			}
			throw new ApiError(res.status, msg || `HTTP ${res.status}`);
		}
		if (res.status === 204) return undefined as T;
		const text = await res.text();
		return (text ? JSON.parse(text) : undefined) as T;
	}

	version = () => this.request<{ version: string; meta: boolean }>('GET', '/version');
	configs = () => this.request<Record<string, unknown>>('GET', '/configs');
	patchConfigs = (patch: Record<string, unknown>) => this.request('PATCH', '/configs', patch);

	proxies = () => this.request<{ proxies: Record<string, Proxy> }>('GET', '/proxies');
	selectProxy = (group: string, name: string) =>
		this.request('PUT', `/proxies/${enc(group)}`, { name });
	unfixProxy = (group: string) => this.request('DELETE', `/proxies/${enc(group)}`);
	proxyDelay = (name: string, url: string, timeout: number) =>
		this.request<{ delay: number }>(
			'GET',
			`/proxies/${enc(name)}/delay?url=${encodeURIComponent(url)}&timeout=${timeout}`,
			undefined,
			timeout + 3000
		);
	groupDelay = (group: string, url: string, timeout: number) =>
		this.request<Record<string, number>>(
			'GET',
			`/group/${enc(group)}/delay?url=${encodeURIComponent(url)}&timeout=${timeout}`,
			undefined,
			timeout + 5000
		);

	providers = () => this.request<{ providers: Record<string, ProxyProvider> }>('GET', '/providers/proxies');
	updateProvider = (name: string) =>
		this.request('PUT', `/providers/proxies/${enc(name)}`, undefined, 60000);
	healthcheckProvider = (name: string) =>
		this.request('GET', `/providers/proxies/${enc(name)}/healthcheck`, undefined, 60000);

	closeConnection = (id: string) => this.request('DELETE', `/connections/${enc(id)}`);
	closeAllConnections = () => this.request('DELETE', '/connections');

	/** Open a reconnecting WebSocket stream. Returns a function that closes it. */
	stream<T>(path: string, onMessage: (data: T) => void): () => void {
		let ws: WebSocket | null = null;
		let closed = false;
		let retry: ReturnType<typeof setTimeout> | undefined;
		const sep = path.includes('?') ? '&' : '?';
		const url = `${this.base.replace(/^http/, 'ws')}${path}${sep}token=${encodeURIComponent(this.secret)}`;

		const connect = () => {
			ws = new WebSocket(url);
			ws.onmessage = (ev) => {
				try {
					onMessage(JSON.parse(ev.data));
				} catch {
					/* ignore malformed frame */
				}
			};
			ws.onclose = () => {
				if (!closed) retry = setTimeout(connect, 1000);
			};
		};
		connect();
		return () => {
			closed = true;
			clearTimeout(retry);
			ws?.close();
		};
	}
}

const enc = encodeURIComponent;
