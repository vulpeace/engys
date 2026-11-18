import { backend, type ProcessEntry } from '#lib/api/backend.ts';

let cached: ProcessEntry[] | null = null;
let expires = 0;
let pending: Promise<ProcessEntry[]> | null = null;
export function installedApps(): Promise<ProcessEntry[]> {
    if (cached && Date.now() < expires) return Promise.resolve(cached);
    if (pending) return pending;
    pending = backend.processesList().then((apps) => {
        cached = apps;
        expires = Date.now() + 30_000;
        return apps;
    }).finally(() => { pending = null; });
    return pending;
}

const icons = new Map<string, Promise<string | null>>();
export function installedAppIcon(packageName: string): Promise<string | null> {
    let icon = icons.get(packageName);
    if (!icon) {
        icon = backend.installedAppIcon(packageName).catch(() => { icons.delete(packageName); return null; });
        icons.set(packageName, icon);
    }
    return icon;
}
