// The live download list: one REST fetch plus a server-sent event stream that keeps it fresh.
import { api } from '../api/client';
import type { AppEvent, Category, Settings } from '../api/types';
import type { Download } from '../api/types';
import { applyEvent } from '../util/downloads';
import { toast } from './toast.svelte';

export type Connection = 'connecting' | 'live' | 'offline';

interface Hooks {
  /** The stream was refused (usually 401). Re-check the session. */
  onClosed: () => void;
  /** Another client changed the settings. */
  onSettings: (s: Settings) => void;
}

const EVENT_NAMES = ['download', 'progress', 'removed', 'settings'] as const;
const RETRY_MS = 4000;

class DownloadsStore {
  items = $state.raw<Download[]>([]);
  loaded = $state(false);
  connection = $state<Connection>('connecting');
  loadError = $state<string | null>(null);

  #source: EventSource | null = null;
  #retry: ReturnType<typeof setTimeout> | null = null;
  #hooks: Hooks | null = null;

  /** Open the stream (idempotent). Subscribing first means no update is missed between
   *  the list fetch and the first event. */
  start(hooks: Hooks): void {
    this.#hooks = hooks;
    if (this.#source) return;
    this.connection = 'connecting';
    const es = new EventSource('/api/events');
    this.#source = es;

    es.onopen = () => {
      this.connection = 'live';
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
    if (this.#retry) clearTimeout(this.#retry);
    this.#retry = null;
    this.items = [];
    this.loaded = false;
    this.connection = 'connecting';
  }

  async reload(): Promise<void> {
    try {
      this.items = await api.listDownloads();
      this.loaded = true;
      this.loadError = null;
    } catch (e) {
      this.loadError = e instanceof Error ? e.message : 'Cannot load downloads';
    }
  }

  // ---- commands: the server answers with events, so the list is not patched here ------
  // (A command's response can be older than the event that followed it.)

  pause = (id: number) => this.#run(() => api.pause(id));
  resume = (id: number) => this.#run(() => api.resume(id));
  cancel = (id: number) => this.#run(() => api.cancel(id));
  remove = (id: number) => this.#run(() => api.remove(id));
  setCategory = (id: number, category: Category) => this.#run(() => api.setCategory(id, category));

  /** Remove every completed/failed/cancelled entry from the list (files stay on disk). */
  async clearFinished(): Promise<void> {
    const finished = this.items.filter((d) => ['completed', 'failed', 'cancelled'].includes(d.status));
    await Promise.all(finished.map((d) => this.#run(() => api.remove(d.id))));
  }

  async #run(call: () => Promise<unknown>): Promise<boolean> {
    try {
      await call();
      if (this.connection !== 'live') await this.reload();
      return true;
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Something went wrong');
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
    }, RETRY_MS);
  }
}

export const downloads = new DownloadsStore();
