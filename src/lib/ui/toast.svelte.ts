export type Toast = { id: number; kind: 'info' | 'success' | 'error'; message: string };

let nextId = 1;

/** Minimal global toast queue. Use `toast.error(e)` in catch blocks. */
class ToastStore {
	items = $state<Toast[]>([]);

	push(kind: Toast['kind'], message: string, ms = kind === 'error' ? 7000 : 3000) {
		const id = nextId++;
		this.items.push({ id, kind, message });
		setTimeout(() => this.dismiss(id), ms);
	}
	info = (m: string) => this.push('info', m);
	success = (m: string) => this.push('success', m);
	error = (e: unknown) => this.push('error', e instanceof Error ? e.message : String(e));
	dismiss(id: number) {
		this.items = this.items.filter((t) => t.id !== id);
	}
}

export const toast = new ToastStore();
