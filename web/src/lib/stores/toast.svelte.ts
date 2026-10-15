// Short messages that disappear on their own.

export type ToastKind = 'info' | 'success' | 'warning' | 'error';
export interface Toast {
  id: number;
  kind: ToastKind;
  message: string;
}

const LIFETIME_MS: Record<ToastKind, number> = { info: 3000, success: 2500, warning: 6000, error: 8000 };
/** More than this at once would cover the page; the oldest make way. */
const MAX_VISIBLE = 3;

class ToastStore {
  items = $state<Toast[]>([]);
  #next = 1;

  push(kind: ToastKind, message: string): void {
    const id = this.#next++;
    this.items = [...this.items, { id, kind, message }].slice(-MAX_VISIBLE);
    setTimeout(() => this.dismiss(id), LIFETIME_MS[kind]);
  }

  info = (message: string) => this.push('info', message);
  success = (message: string) => this.push('success', message);
  warning = (message: string) => this.push('warning', message);
  error = (message: string) => this.push('error', message);

  dismiss(id: number): void {
    this.items = this.items.filter((t) => t.id !== id);
  }
}

export const toast = new ToastStore();
