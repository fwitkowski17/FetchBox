// The torrent list: one REST fetch, kept fresh by the `torrent` events of the shared event stream
// (the stream itself is opened by the downloads store). Also which torrent the details panel shows
// and the speed history behind its chart.
import { api } from '../api/client';
import type { AppEvent, Torrent } from '../api/types';
import { i18n } from '../i18n';
import {
  applyTorrentEvent,
  pushSample,
  type SortDir,
  type SpeedSample,
  type TorrentSortKey,
  TORRENT_SORT_KEYS,
  isTorrentGroup,
  type TorrentGroup,
} from '../util/torrents';
import { readJson, writeJson } from '../util/storage';
import { toast } from './toast.svelte';

const KEY = 'fetchbox.torrents';

interface Stored {
  sort: TorrentSortKey;
  dir: SortDir;
  /** The status filter of the side panel. */
  group: TorrentGroup;
}

function isStored(v: unknown): v is Stored {
  if (typeof v !== 'object' || v === null) return false;
  const s = v as Record<string, unknown>;
  return (
    typeof s.sort === 'string' &&
    (TORRENT_SORT_KEYS as readonly string[]).includes(s.sort) &&
    (s.dir === 'asc' || s.dir === 'desc') &&
    // A save from before the filter has none.
    (s.group === undefined || isTorrentGroup(s.group))
  );
}

class TorrentsStore {
  items = $state.raw<Torrent[]>([]);
  loaded = $state(false);
  loadError = $state.raw<unknown>(null);
  /** The torrent whose details are open (the panel at the bottom), if any. */
  selectedId = $state<number | null>(null);
  query = $state('');
  #stored = $state<Stored>(readJson<Stored>(KEY, { sort: 'added', dir: 'desc', group: 'all' }, isStored));
  /** Speed samples by torrent id, from the events seen since the page was opened. */
  history = $state.raw<Record<number, readonly SpeedSample[]>>({});

  get sort(): TorrentSortKey {
    return this.#stored.sort;
  }
  get dir(): SortDir {
    return this.#stored.dir;
  }

  get group(): TorrentGroup {
    return this.#stored.group ?? 'all';
  }

  setGroup(group: TorrentGroup): void {
    this.#stored = { ...this.#stored, group };
    writeJson(KEY, this.#stored);
  }

  get selected(): Torrent | null {
    return this.items.find((t) => t.id === this.selectedId) ?? null;
  }

  setSort(sort: TorrentSortKey): void {
    this.#stored = { ...this.#stored, sort, dir: sort === 'name' ? 'asc' : 'desc' };
    writeJson(KEY, this.#stored);
  }

  toggleDir(): void {
    this.#stored = { ...this.#stored, dir: this.#stored.dir === 'asc' ? 'desc' : 'asc' };
    writeJson(KEY, this.#stored);
  }

  /** Open the details of a torrent; the same one again closes them. */
  toggleSelected(id: number): void {
    this.selectedId = this.selectedId === id ? null : id;
  }

  closeDetails(): void {
    this.selectedId = null;
  }

  async reload(): Promise<void> {
    try {
      this.items = await api.listTorrents();
      this.loaded = true;
      this.loadError = null;
      this.#dropStale();
    } catch (e) {
      this.loadError = e;
    }
  }

  /** An event from the shared stream (`torrent`, `torrent_removed`). */
  received(ev: AppEvent): void {
    this.items = applyTorrentEvent(this.items, ev);
    if (ev.type === 'torrent') {
      const t = ev.torrent;
      const sample = { at: Date.now(), down: t.down_speed, up: t.up_speed };
      this.history = { ...this.history, [t.id]: pushSample(this.history[t.id] ?? [], sample) };
    } else if (ev.type === 'torrent_removed') {
      this.#dropStale();
    }
  }

  /** Forget everything (sign-out). */
  clear(): void {
    this.items = [];
    this.loaded = false;
    this.loadError = null;
    this.selectedId = null;
    this.history = {};
  }

  // ---- commands: the stream brings the result, so the list is not patched here ----------------

  pause = (id: number) => this.#run(() => api.pauseTorrent(id));
  resume = (id: number) => this.#run(() => api.resumeTorrent(id));
  startNow = (id: number) => this.#run(() => api.startTorrentNow(id));
  remove = (id: number, deleteFiles: boolean) => this.#run(() => api.removeTorrent(id, deleteFiles));

  /** Add by magnet link. Throws: the dialog shows what went wrong next to the field. */
  async addMagnet(magnet: string, disk?: string): Promise<Torrent> {
    const t = await api.addTorrentMagnet(magnet.trim(), disk);
    this.#adopt(t);
    return t;
  }

  /** Add the bytes of a `.torrent` file. Throws like `addMagnet`. */
  async addFile(bytes: ArrayBuffer, disk?: string): Promise<Torrent> {
    const t = await api.addTorrentFile(bytes, disk);
    this.#adopt(t);
    return t;
  }

  /** The answer to an add is the only copy of the new torrent until its event arrives. */
  #adopt(t: Torrent): void {
    if (!this.items.some((x) => x.id === t.id)) this.items = [t, ...this.items];
  }

  async #run(call: () => Promise<unknown>): Promise<boolean> {
    try {
      await call();
      return true;
    } catch (e) {
      toast.error(i18n.errorText(e));
      return false;
    }
  }

  /** History and selection of torrents that are gone. */
  #dropStale(): void {
    const ids = new Set(this.items.map((t) => t.id));
    if (this.selectedId !== null && !ids.has(this.selectedId)) this.selectedId = null;
    const keep = Object.keys(this.history).filter((id) => ids.has(Number(id)));
    if (keep.length !== Object.keys(this.history).length) {
      this.history = Object.fromEntries(keep.map((id) => [id, this.history[Number(id)] ?? []]));
    }
  }
}

export const torrents = new TorrentsStore();
