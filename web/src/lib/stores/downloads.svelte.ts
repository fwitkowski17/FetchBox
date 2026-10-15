// The live download list: one REST fetch plus a server-sent event stream that keeps it fresh.
import { ApiError, api } from '../api/client';
import type { AppEvent, Category, Settings } from '../api/types';
import type { Download } from '../api/types';
import type { BulkResult } from '../util/bulk';
import { applyEvent } from '../util/downloads';
import { retryDelay } from '../util/retry';
import { i18n } from '../i18n';
import { toast } from './toast.svelte';

export type Connection = 'connecting' | 'live' | 'offline';

interface Hooks {
  /** The stream was refused (usually 401). Re-check the session. */
  onClosed: () => void;
  /** Another client changed the settings. */
  onSettings: (s: Settings) => void;
}

const EVENT_NAMES = ['download', 'progress', 'removed', 'settings'] as const;

class DownloadsStore {
  items = $state.raw<Download[]>([]);
  loaded = $state(false);
  connection = $state<Connection>('connecting');
  /** The error from the last failed load (shown through `i18n.errorText`). */
  loadError = $state.raw<unknown>(null);
  /** Id of a download whose resume was refused because the disk is still full. A dialog with the
   *  space gauge explains it (instead of a toast) until the user closes it or the resume works. */
  noSpaceFor = $state<number | null>(null);

  #source: EventSource | null = null;
  #retry: ReturnType<typeof setTimeout> | null = null;
  #attempt = 0;
  #hooks: Hooks | null = null;
  #watching = false;

  /** Open the stream (idempotent). Subscribing first means no update is missed between
   *  the list fetch and the first event. */
  start(hooks: Hooks): void {
    this.#hooks = hooks;
    if (this.#source) return;
    this.connection = 'connecting';
    const es = new EventSource('/api/events');
    this.#source = es;
    this.#watchTheTab();
    // The list does not wait for the stream to open: both travel at once, and the reload on open
    // below still closes any gap between them.
    if (!this.loaded) void this.reload();

    es.onopen = () => {
      this.connection = 'live';
      this.#attempt = 0;
      // Also after a reconnect: events sent while we were away are gone, the list is not.
      void this.reload();
    };
    es.onerror = () => {
      if (es.readyState === EventSource.CLOSED) {
        // The browser gave up (HTTP error such as 401); it will not retry by itself.
        this.#close();
        this.connection = 'offline';
        this.#hooks?.onClosed();
        this.#scheduleRetry();
      } else {
        this.connection = 'connecting'; // the browser is reconnecting
      }
    };
    for (const name of EVENT_NAMES) {
      es.addEventListener(name, (e) => this.#onMessage((e as MessageEvent<string>).data));
    }
    es.addEventListener('resync', () => void this.reload());
  }

  /** Close the stream and forget everything (logout). */
  stop(): void {
    this.#close();
    this.#unwatchTheTab();
    this.#attempt = 0;
    if (this.#retry) clearTimeout(this.#retry);
    this.#retry = null;
    this.items = [];
    this.noSpaceFor = null;
    this.loaded = false;
    this.connection = 'connecting';
  }

  async reload(): Promise<void> {
    try {
      this.items = await api.listDownloads();
      this.loaded = true;
      this.loadError = null;
    } catch (e) {
      this.loadError = e;
    }
  }

  // ---- commands: the server answers with events, so the list is not patched here ------
  // (A command's response can be older than the event that followed it.)

  pause = (id: number) => this.#run(() => api.pause(id));
  resume = async (id: number): Promise<boolean> => {
    const ok = await this.#run(() => api.resume(id), id);
    if (ok && this.noSpaceFor === id) this.noSpaceFor = null;
    return ok;
  };
  cancel = (id: number) => this.#run(() => api.cancel(id));
  remove = (id: number, deleteFile = false) => this.#run(() => api.remove(id, deleteFile));
  setCategory = (id: number, category: Category) => this.#run(() => api.setCategory(id, category));
  setArchived = (id: number, archived: boolean) => this.#run(() => api.setArchived(id, archived));
  setSpeedLimit = (id: number, bytesPerSecond: number) =>
    this.#run(() => api.setSpeedLimit(id, bytesPerSecond));

  // ---- the same commands for a whole selection. No toast per download: the caller shows one
  // summary. The list follows the events as always.
  removeMany = (ids: readonly number[], deleteFile: boolean) =>
    this.#many(ids, (id) => api.remove(id, deleteFile));
  setCategoryMany = (ids: readonly number[], category: Category) =>
    this.#many(ids, (id) => api.setCategory(id, category));
  setArchivedMany = (ids: readonly number[], archived: boolean) =>
    this.#many(ids, (id) => api.setArchived(id, archived));

  async #many(ids: readonly number[], call: (id: number) => Promise<unknown>): Promise<BulkResult> {
    const results = await Promise.allSettled(ids.map(call));
    const failures = results.filter((r): r is PromiseRejectedResult => r.status === 'rejected');
    if (this.connection !== 'live') await this.reload();
    return { done: results.length - failures.length, failed: failures.length, firstError: failures[0]?.reason };
  }

  /** Remove every completed/failed/cancelled entry from the list (files stay on disk). */
  async clearFinished(): Promise<void> {
    // Archived entries were put away on purpose: clearing the list leaves them alone.
    const finished = this.items.filter(
      (d) => !d.archived && ['completed', 'failed', 'cancelled'].includes(d.status),
    );
    await Promise.all(finished.map((d) => this.#run(() => api.remove(d.id))));
  }

  /** `resumeOf`: the download this call tries to resume; a "disk full" refusal then opens the
   *  space dialog instead of showing a toast. */
  async #run(call: () => Promise<unknown>, resumeOf?: number): Promise<boolean> {
    try {
      await call();
      if (this.connection !== 'live') await this.reload();
      return true;
    } catch (e) {
      if (resumeOf !== undefined && e instanceof ApiError && e.status === 507 && e.space) {
        this.noSpaceFor = resumeOf;
        return false;
      }
      toast.error(i18n.errorText(e));
      return false;
    }
  }

  #onMessage(data: string): void {
    let ev: AppEvent;
    try {
      ev = JSON.parse(data) as AppEvent;
    } catch {
      return;
    }
    if (ev.type === 'settings') {
      this.#hooks?.onSettings(ev.settings);
      return;
    }
    this.items = applyEvent(this.items, ev);
  }

  #close(): void {
    this.#source?.close();
    this.#source = null;
  }

  #scheduleRetry(): void {
    if (this.#retry) return;
    this.#retry = setTimeout(() => {
      this.#retry = null;
      if (this.#hooks) this.start(this.#hooks);
    }, retryDelay(this.#attempt++));
  }

  // A phone that was asleep, a laptop that changed network: the browser may sit on a dead stream or
  // wait out its own retry timer. Coming back to the tab (or the network coming back) does not
  // wait: reconnect at once if the stream is not live, and refresh the list if it is, since a
  // stream that was frozen in the background missed whatever happened meanwhile.
  #watchTheTab(): void {
    if (this.#watching || typeof document === 'undefined') return;
    this.#watching = true;
    document.addEventListener('visibilitychange', this.#onVisible);
    window.addEventListener('online', this.#onBack);
  }

  #unwatchTheTab(): void {
    if (!this.#watching) return;
    this.#watching = false;
    document.removeEventListener('visibilitychange', this.#onVisible);
    window.removeEventListener('online', this.#onBack);
  }

  #onVisible = (): void => {
    if (!document.hidden) this.#onBack();
  };

  #onBack = (): void => {
    if (!this.#hooks) return;
    if (this.connection === 'live') {
      void this.reload();
      return;
    }
    this.#close();
    if (this.#retry) clearTimeout(this.#retry);
    this.#retry = null;
    this.#attempt = 0;
    this.start(this.#hooks);
  };
}

export const downloads = new DownloadsStore();
