import type { Connection, ConnectionsSnapshot, Memory, Mihomo, Traffic } from '#lib/api/mihomo.ts';

export const HISTORY_POINTS = 90;

export type ConnRecord = {
	conn: Connection;
	/** ms timestamp when the connection disappeared from the core's list. */
	closedAt: number | null;
	upSpeed: number;
	downSpeed: number;
};

/**
 * Live data streamed from the running core over WebSockets:
 * traffic rate, memory, and connections.
 *
 * mihomo only reports *open* connections, so the request history is built here:
 * connections that vanish from a snapshot are marked closed and kept until the
 * history limit is reached.
 */
class LiveStore {
	traffic = $state<Traffic>({ up: 0, down: 0 });
	memory = $state(0);
	uploadTotal = $state(0);
	downloadTotal = $state(0);
	activeCount = $state(0);

	/** Rolling samples for the bandwidth chart (unix seconds, bytes/s). */
	samples = $state.raw<{ t: number[]; up: number[]; down: number[] }>({ t: [], up: [], down: [] });

	/** Newest first. Replaced wholesale on every tick (raw = no deep proxying). */
	records = $state.raw<ConnRecord[]>([]);

	historyLimit = 2000;
	#map = new Map<string, ConnRecord>();
	#stops: (() => void)[] = [];
	#streamGeneration = 0;
	#trafficTotals = { up: false, down: false };

	connect(api: Mihomo) {
		this.disconnect();
		const generation = this.#streamGeneration;
		const subscribe = <T>(path: string, callback: (value: T) => void) => api.stream<T>(path, (value) => {
			if (generation === this.#streamGeneration) callback(value);
		});
		this.#stops.push(subscribe<Traffic>('/traffic', (t) => this.#onTraffic(t)));
		// The first /memory frame is always 0; skip it so the tile doesn't flash.
		this.#stops.push(
			subscribe<Memory>('/memory', (m) => {
				if (m.inuse) this.memory = m.inuse;
			}));
		this.#stops.push(
			subscribe<ConnectionsSnapshot>('/connections?interval=1000', (s) => this.#onConnections(s))
		);
	}

	disconnect() {
		// Invalidate callbacks before closing sockets, including messages already queued.
		this.#streamGeneration++;
		this.#stops.forEach((s) => s());
		this.#stops = [];
		this.traffic = { up: 0, down: 0 };
		this.memory = 0;
		this.activeCount = 0;
		// Mark everything closed but keep the history visible after stopping.
		const now = Date.now();
		for (const r of this.#map.values()) {
			if (!r.closedAt) Object.assign(r, { closedAt: now, upSpeed: 0, downSpeed: 0 });
		}
		this.#publish();
	}

	/** New core session: wipe session counters and chart. */
	resetSession() {
		this.#trafficTotals = { up: false, down: false };
		this.uploadTotal = 0;
		this.downloadTotal = 0;
		this.samples = { t: [], up: [], down: [] };
	}

	clearConnections() {
		this.#map.clear();
		this.records = [];
		this.activeCount = 0;
	}

	clearHistory() {
		for (const [id, r] of this.#map) if (r.closedAt) this.#map.delete(id);
		this.#publish();
	}

	#onTraffic(t: Traffic) {
		this.traffic = t;
		this.#updateTotals('traffic', t.upTotal, t.downTotal);
		const s = this.samples;
		const cut = s.t.length >= HISTORY_POINTS ? 1 : 0;
		this.samples = {
			t: [...s.t.slice(cut), Date.now() / 1000],
			up: [...s.up.slice(cut), t.up],
			down: [...s.down.slice(cut), t.down]
		};
	}

	#onConnections(snap: ConnectionsSnapshot) {
		const now = Date.now();
		const conns = snap.connections ?? [];
		const seen = new Set<string>();
		for (const c of conns) {
			seen.add(c.id);
			const prev = this.#map.get(c.id);
			if (prev) {
				prev.closedAt = null;
				prev.upSpeed = Math.max(0, c.upload - prev.conn.upload);
				prev.downSpeed = Math.max(0, c.download - prev.conn.download);
				prev.conn = c;
			} else {
				this.#map.set(c.id, { conn: c, closedAt: null, upSpeed: 0, downSpeed: 0 });
			}
		}
		for (const [id, r] of this.#map) {
			if (!r.closedAt && !seen.has(id)) Object.assign(r, { closedAt: now, upSpeed: 0, downSpeed: 0 });
		}
		// Map keeps insertion order (oldest first): evict the oldest closed entries.
		if (this.#map.size > this.historyLimit) {
			let excess = this.#map.size - this.historyLimit;
			for (const [id, r] of this.#map) {
				if (excess <= 0) break;
				if (r.closedAt) {
					this.#map.delete(id);
					excess--;
				}
			}
		}
		// Fallback totals for cores that don't send upTotal/downTotal on /traffic.
		this.#updateTotals('connections', snap.uploadTotal, snap.downloadTotal);
		this.activeCount = conns.length;
		this.#publish();
	}

	#updateTotals(source: 'traffic' | 'connections', up?: number, down?: number) {
		for (const [direction, value] of [['up', up], ['down', down]] as const) {
			if (value === undefined || !Number.isFinite(value) || value < 0) continue;
			if (source === 'traffic') this.#trafficTotals[direction] = true;
			else if (this.#trafficTotals[direction]) continue;
			if (direction === 'up') this.uploadTotal = value;
			else this.downloadTotal = value;
		}
	}

	#publish() {
		// Records are mutated in place; spread into new objects so keyed rows re-render.
		this.records = [...this.#map.values()].reverse().map((r) => ({ ...r }));
	}
}

export const live = new LiveStore();
