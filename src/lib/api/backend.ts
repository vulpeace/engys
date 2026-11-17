// Typed wrappers around the Rust commands in src-tauri/src/commands.rs.
import { invoke } from '@tauri-apps/api/core';

export type CoreStatus = {
	running: boolean;
	pid: number | null;
	controller: string;
	secret: string;
	startedAt: number | null;
	profileId: string | null;
	binary: string | null;
	lastError: string | null;
};

export type LoginStatus = {
	enabled: boolean;
	supported: boolean;
	needsRepair: boolean;
	detail: string;
};

export type Settings = {
	overrides: string[];
	corePath: string | null;
	controllerPort: number;
	secret: string;
	fallbackSecret: string;
	/** null = use the profile's `mixed-port` (if any). */
	mixedPort: number | null;
	allowLan: boolean;
	ipv6: boolean;
	logLevel: 'silent' | 'error' | 'warning' | 'info' | 'debug';
	mode: 'rule' | 'global' | 'direct';
	delayTestUrl: string;
	delayTestTimeout: number;
	requestHistoryLimit: number;
	subscriptionUserAgent: string;
	theme: 'system' | 'light' | 'dark';
	minimizeToTray: boolean;
	tun: TunSettings;
};

export type TunSettings = {
	enable: boolean;
	stack: 'system' | 'gvisor' | 'mixed' | 'mips';
	autoRoute: boolean;
	strictRoute: boolean;
	autoDetectInterface: boolean;
	dnsHijack: string[];
	/** '' = core default. */
	dnsMode: '' | 'disabled' | 'native' | 'hijack';
	device: string;
	mtu: number | null;
	dnsNameservers: string[];
};

export type TunPermission = {
	granted: boolean;
	method: 'root' | 'capabilities' | 'administrator' | 'unsupported';
	canGrant: boolean;
	detail: string;
};

export type SubscriptionInfo = {
	upload: number;
	download: number;
	total: number;
	expire: number | null;
};

export type Profile = {
	id: string;
	name: string;
	kind: 'inline' | 'file' | 'remote';
	source: string | null;
	createdAt: number;
	updatedAt: number;
	subscription: SubscriptionInfo | null;
	updateIntervalHours: number | null;
	updateIntervalMinutes: number;
	updateIntervalSource: 'default' | 'client' | 'subscription';
	lastRefreshedAt: number | null;
	lastRefreshAttemptAt: number | null;
	lastRefreshError: string | null;
};

export type ProfileIndex = { active: string | null; profiles: Profile[] };

export type AppMatchKind = 'processName' | 'processPath' | 'processNameRegex' | 'processPathRegex';
export type AppRule = { id: string; kind: AppMatchKind; value: string; target: string };
export type AppRulesView = {
    rules: AppRule[]; targets: string[]; activeProfile: string | null;
    selectThroughProxy: boolean; inferredSelectThroughProxy: boolean | null; orderOverride: boolean | null;
};

export type CustomProxyGroup = {
	id: string; name: string; kind: 'select' | 'url-test' | 'fallback' | 'load-balance';
	proxies: string[]; providers: string[]; includeAllProxies: boolean; includeAllProviders: boolean;
	filter: string; url: string; interval: number; strategy: 'consistent-hashing' | 'round-robin' | 'sticky-sessions';
};
export type CustomGroupsView = {
	profileId: string | null; profileName: string | null; groups: CustomProxyGroup[]; members: string[]; providers: string[];
};

export type RoutingEntry = { id: string; sourceIndex: number | null; rule: string };
export type RoutingView = {
    profileId: string | null; profileName: string | null; sourceRules: string[]; entries: RoutingEntry[];
    providersYaml: string; inheritedProvidersYaml: string; revision: string;
};

export type ProcessEntry = { pid: number; name: string; path: string | null };

export type UpdateInfo = {
	currentVersion: string | null;
	currentPath: string | null;
	managed: boolean;
	latestVersion: string;
	assetName: string;
	updateAvailable: boolean;
};

export type CoreBinaryInfo = { path: string | null; version: string | null; error: string | null };

export const backend = {
    routingGet: () => invoke<RoutingView>('routing_get'),
    routingSave: (profileId: string, revision: string, entries: RoutingEntry[], providersYaml: string, editInherited: boolean) =>
        invoke<RoutingView>('routing_save', { profileId, revision, entries, providersYaml, editInherited }),
	customGroupsGet: () => invoke<CustomGroupsView>('custom_groups_get'),
	customGroupsSave: (profileId: string, groups: CustomProxyGroup[]) => invoke<CustomGroupsView>('custom_groups_save', { profileId, groups }),
	clientMemory: () => invoke<number | null>('client_memory'),
	loginStatus: () => invoke<LoginStatus>('login_status'),
	loginSet: (enabled: boolean) => invoke<LoginStatus>('login_set', { enabled }),
	coreStatus: () => invoke<CoreStatus>('core_status'),
	coreStart: () => invoke<CoreStatus>('core_start'),
	coreStop: () => invoke<CoreStatus>('core_stop'),
	coreLogs: () => invoke<string[]>('core_logs'),
	coreReload: () => invoke<void>('core_reload'),
	coreBinaryInfo: () => invoke<CoreBinaryInfo>('core_binary_info'),
	workDir: () => invoke<string>('work_dir'),
	coreCheckUpdate: () => invoke<UpdateInfo>('core_check_update'),
	/** Downloads the latest release; emits `core-update-progress`. Returns the installed tag. */
	coreInstallUpdate: () => invoke<string>('core_install_update'),

	settingsGet: () => invoke<Settings>('settings_get'),
	settingsInherited: () => invoke<Settings>('settings_inherited'),
	settingsSave: (settings: Settings) => invoke<Settings>('settings_save', { settings }),

	profilesList: () => invoke<ProfileIndex>('profiles_list'),
	profileCreateInline: (name: string, content: string) =>
		invoke<Profile>('profile_create_inline', { name, content }),
	profileImportFile: (path: string, name?: string) =>
		invoke<Profile>('profile_import_file', { path, name }),
	profileAddRemote: (url: string, name?: string, updateIntervalMinutes = 0, intervalOverridden = false) =>
		invoke<Profile>('profile_add_remote', { url, name, updateIntervalMinutes, intervalOverridden }),
	profileUpdateRemote: (id: string) => invoke<Profile>('profile_update_remote', { id }),
	profileResetUpdateInterval: (id: string) => invoke<Profile>('profile_reset_update_interval', { id }),
	profileSetUpdateInterval: (id: string, minutes: number) => invoke<Profile>('profile_set_update_interval', { id, minutes }),
	profileRead: (id: string) => invoke<string>('profile_read', { id }),
	profileReadFinal: (id: string) => invoke<string>('profile_read_final', { id }),
	profileSave: (id: string, content: string) => invoke<void>('profile_save', { id, content }),
	profileRename: (id: string, name: string) => invoke<Profile>('profile_rename', { id, name }),
	profileDelete: (id: string) => invoke<void>('profile_delete', { id }),
	profileActivate: (id: string) => invoke<void>('profile_activate', { id }),

	appRulesGet: () => invoke<AppRulesView>('app_rules_get'),
	appRulesSave: (rules: AppRule[], orderOverride: boolean | null) => invoke<AppRulesView>('app_rules_save', { rules, orderOverride }),
	processesList: () => invoke<ProcessEntry[]>('processes_list'),

	tunPermission: () => invoke<TunPermission>('tun_permission'),
	/** Linux: polkit prompt. Windows: relaunches the app elevated (this instance exits). */
	tunGrantPermission: () => invoke<TunPermission>('tun_grant_permission')
};
