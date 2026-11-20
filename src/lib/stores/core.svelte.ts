import { listen } from '@tauri-apps/api/event';
import { overrideKeys } from '#lib/settings-overrides.ts';
import {
	backend,
	type CoreBinaryInfo,
	type CoreStatus,
	type Settings,
	type TunPermission
} from '#lib/api/backend.ts';
import { Mihomo } from '#lib/api/mihomo.ts';

const LOG_CAP = 1000;

/**
 * Core process state + app settings. The Rust side is the source of truth;
 * this mirrors it and listens for `core-status` / `core-log` events.
 */
class CoreStore {
	platform = $state('');
	android = $derived(this.platform === 'android');
	status = $state<CoreStatus | null>(null);
	settings = $state<Settings | null>(null);
	logs = $state<string[]>([]);
	busy = $state(false);

	/** Whether the core can create a TUN device (null = unknown / no binary). */
	tunPermission = $state<TunPermission | null>(null);
	/** TUN state as reported by the running core — the source of truth. */
	tunActive = $state(false);
	tunBusy = $state(false);

	/** The core binary that would be launched, and its `-v` output. */
	binary = $state<CoreBinaryInfo | null>(null);
	/** Parsed core version, e.g. [1, 19, 34]. */
	version = $derived(parseVersion(this.binary?.version));

	running = $derived(this.status?.running ?? false);
	/** API client for the running core (null when stopped). */
	api = $derived(
		this.status?.running ? new Mihomo(this.status.controller, this.status.secret) : null
	);

	#initialized = false;

	async init() {
		if (this.#initialized) return;
		this.#initialized = true;
		this.platform = await backend.platformInfo();
		await listen<CoreStatus>('core-status', (e) => {
			this.status = e.payload;
			void this.refreshSettings();
			if (!e.payload.running) this.tunActive = false;
			else {
				void this.refreshTunActive();
				void this.refreshTunPermission();
			}
		});
		await listen('profiles-changed', () => { void this.refreshSettings(); });
		await listen<string>('core-log', (e) => {
			this.logs.push(e.payload);
			if (this.logs.length > LOG_CAP) this.logs.splice(0, this.logs.length - LOG_CAP);
		});
		[this.status, this.settings, this.logs] = await Promise.all([
			backend.coreStatus(),
			backend.settingsGet(),
			backend.coreLogs()
		]);
		await Promise.all([this.refreshTunPermission(), this.refreshTunActive(), this.refreshBinary()]);
        if (this.android) {
            const refresh = async () => {
                if (document.visibilityState !== 'visible') return;
                try {
                    this.status = await backend.coreStatus();
                    await this.refreshTunActive();
                    this.logs = await backend.coreLogs();
                } catch { /* The next poll retries after activity resume. */ }
            };
            document.addEventListener('visibilitychange', () => { void refresh(); });
            setInterval(() => { void refresh(); }, 2000);
        }
	}

	async start() {
		this.busy = true;
		try {
			this.logs = [];
            if (this.android && this.settings?.tun.enable) await this.ensureVpnPermission();
			this.status = await backend.coreStart();
		} finally {
			this.busy = false;
		}
		if (this.settings?.tun.enable) {
			await this.refreshTunActive();
			if (!this.tunActive) {
				throw new Error(`Core started, but TUN failed: ${this.#lastTunError()}`);
			}
		}
	}

	async stop() {
		this.busy = true;
		try {
			this.status = await backend.coreStop();
		} finally {
			this.busy = false;
		}
	}

    async ensureVpnPermission() {
        await this.refreshTunPermission();
        if (!this.tunPermission?.granted) this.tunPermission = await backend.tunGrantPermission();
        if (!this.tunPermission?.granted) throw new Error('VPN permission was not granted.');
    }

	/** Persist a settings change. */
	async saveSettings(patch: Partial<Settings>) {
		if (!this.settings) return;
		this.settings = await backend.settingsSave({ ...this.settings, ...patch });
	}

	async refreshSettings() { this.settings = await backend.settingsGet(); }

	async resetTun() {
		const inherited = await backend.settingsInherited();
		await this.setTun(inherited.tun.enable, true);
	}

	async resetMode() {
		const inherited = await backend.settingsInherited();
		await this.setMode(inherited.mode, true);
	}

	async refreshBinary() {
		this.binary = await backend
			.coreBinaryInfo()
			.catch((e) => ({ path: null, version: null, error: String(e) }));
	}

	async refreshTunPermission() {
		this.tunPermission = await backend.tunPermission().catch(() => null);
	}

	async refreshTunActive() {
		const api = this.api;
		if (!api) {
			this.tunActive = false;
			return;
		}
		try {
            if (this.android) { this.tunActive = await backend.androidVpnActive(); return; }
			const cfg = await api.configs();
			this.tunActive = !!(cfg.tun as { enable?: boolean } | undefined)?.enable;
		} catch {
			this.tunActive = false;
		}
	}

	/** Linux: one-time password prompt. Windows: the app restarts elevated. */
	async grantTunPermission() {
		this.tunBusy = true;
		try {
			this.tunPermission = await backend.tunGrantPermission();
		} finally {
			this.tunBusy = false;
		}
	}

	/**
	 * Turn TUN on/off. Persists the choice and, if the core is running,
	 * hot-reloads it and verifies the core actually brought TUN up — rolling
	 * the setting back if it didn't.
	 */
	async setTun(enable: boolean, inherit = false) {
		if (!this.settings) return;
		const previous = structuredClone($state.snapshot(this.settings));
		this.tunBusy = true;
		try {
			if (enable) {
                if (this.android) await this.ensureVpnPermission();
				await this.refreshTunPermission();
				if (!this.tunPermission?.granted) {
					throw new Error(this.tunPermission?.detail ?? 'TUN mode needs permission first.');
				}
			}
			await this.saveSettings({ overrides: overrideKeys(this.settings, 'tun.enable', !inherit), tun: { ...this.settings.tun, enable } });
			if (!this.running) return;
			try {
				await backend.coreReload();
			} catch (e) {
				await this.saveSettings({ overrides: previous.overrides, tun: { ...this.settings.tun, enable: previous.tun.enable } });
				throw e;
			}
			await this.refreshTunActive();
			if (enable && !this.tunActive && /not permitted|access is denied/i.test(this.#lastTunError())) {
				// Permission was granted after this core process started; file
				// capabilities only apply on exec, so restart it once.
				await this.stop();
				await this.start().catch(() => {});
			}
			if (enable && !this.tunActive) {
				const reason = this.#lastTunError();
				await this.saveSettings({ overrides: previous.overrides, tun: { ...this.settings.tun, enable: previous.tun.enable } });
				await backend.coreReload().catch(() => {});
				throw new Error(`TUN failed to start: ${reason}`);
			}
		} finally {
			this.tunBusy = false;
		}
	}

	#lastTunError(): string {
		const line = [...this.logs].reverse().find((l) => /tun/i.test(l) && /level=(error|fatal)/.test(l));
		const msg = line?.match(/msg="(.*)"/)?.[1];
		return msg ?? line ?? 'see the core log on the Dashboard';
	}

	/** Change the proxy mode live (if running) and persist it for the next start. */
	async setMode(mode: Settings['mode'], inherit = false) {
        if (this.android && this.settings) {
            const previous = structuredClone($state.snapshot(this.settings));
            await this.saveSettings({ mode, overrides: overrideKeys(this.settings, 'mode', !inherit) });
            try { if (this.running) await backend.coreReload(); }
            catch (e) { await this.saveSettings(previous); throw e; }
            return;
        }
		await this.api?.patchConfigs({ mode });
		if (this.settings) await this.saveSettings({ mode, overrides: overrideKeys(this.settings, 'mode', !inherit) });
	}
}

/** "Mihomo Meta v1.19.34 linux amd64 …" -> [1, 19, 34] */
export function parseVersion(s: string | null | undefined): [number, number, number] | null {
	const m = s?.match(/\bv(\d+)\.(\d+)\.(\d+)/);
	return m ? [Number(m[1]), Number(m[2]), Number(m[3])] : null;
}

export function versionAtLeast(v: [number, number, number] | null, min: [number, number, number]) {
	if (!v) return true; // unknown (e.g. alpha build): don't nag
	for (let i = 0; i < 3; i++) if (v[i] !== min[i]) return v[i] > min[i];
	return true;
}

export const core = new CoreStore();
