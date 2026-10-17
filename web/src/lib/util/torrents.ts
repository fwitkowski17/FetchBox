// Pure logic of the torrent list: progress, ratio, filtering, sorting, live events and the speed history.
import type { AppEvent, Torrent, TorrentStatus } from '../api/types';

export const TORRENT_STATUSES: readonly TorrentStatus[] = [
  'metadata',
  'checking',
  'connecting',
  'downloading',
  'seeding',
  'paused',
  'finished',
  'error',
];

/** Statuses in which the torrent holds connections and uses the network. */
export function isActive(t: Pick<Torrent, 'status'>): boolean {
  return t.status !== 'paused' && t.status !== 'finished' && t.status !== 'error';
}

/** 0..1, or `null` while the size is not known (a magnet link before its description). */
export function progressOf(t: Pick<Torrent, 'size' | 'downloaded'>): number | null {
  if (t.size === null || t.size === undefined) return null;
  if (t.size === 0) return 1;
  return Math.min(1, Math.max(0, t.downloaded / t.size));
}

/** Sent over received. `null` when nothing has been received yet (a ratio of "infinity" says nothing). */
export function ratioOf(t: Pick<Torrent, 'uploaded' | 'downloaded'>): number | null {
  if (t.downloaded <= 0) return null;
  return t.uploaded / t.downloaded;
}

/** The torrent hours keep this torrent stopped; it starts by itself when they open. */
export function isHeld(t: Pick<Torrent, 'hold'>): boolean {
  return t.hold === 'window';
}

/** What the main button of a torrent does. A held torrent offers "start now" (a pause button sits next to it). */
export type TorrentAction = 'pause' | 'resume' | 'retry' | 'startNow';

export function primaryAction(t: Pick<Torrent, 'status' | 'hold'>): TorrentAction {
  if (isHeld(t)) return 'startNow';
  if (t.status === 'paused' || t.status === 'finished') return 'resume';
  if (t.status === 'error') return 'retry';
  return 'pause';
}

export function filterTorrents(items: readonly Torrent[], query: string): Torrent[] {
  const q = query.trim().toLowerCase();
  if (q === '') return items.slice();
  return items.filter((t) => t.name.toLowerCase().includes(q) || t.info_hash.startsWith(q));
}

export type TorrentSortKey = 'added' | 'name' | 'size' | 'progress' | 'down' | 'up' | 'status';
export type SortDir = 'asc' | 'desc';

export const TORRENT_SORT_KEYS: readonly TorrentSortKey[] = [
  'added',
  'name',
  'size',
  'progress',
  'down',
  'up',
  'status',
];

/** Sort a copy. Unknown sizes and progress always come last, whatever the direction. */
export function sortTorrents(
  items: readonly Torrent[],
  key: TorrentSortKey,
  dir: SortDir,
  locale: string,
): Torrent[] {
  const sign = dir === 'asc' ? 1 : -1;
  const collator = new Intl.Collator(locale, { numeric: true, sensitivity: 'base' });
  const number = (a: number | null, b: number | null): number => {
    if (a === null && b === null) return 0;
    if (a === null) return 1;
    if (b === null) return -1;
    return (a - b) * sign;
  };
  const order = (t: Torrent): number => TORRENT_STATUSES.indexOf(t.status);
  return items.slice().sort((a, b) => {
    let c: number;
    switch (key) {
      case 'name':
        c = collator.compare(a.name, b.name) * sign;
        break;
      case 'size':
        c = number(a.size ?? null, b.size ?? null);
        break;
      case 'progress':
        c = number(progressOf(a), progressOf(b));
        break;
      case 'down':
        c = (a.down_speed - b.down_speed) * sign;
        break;
      case 'up':
        c = (a.up_speed - b.up_speed) * sign;
        break;
      case 'status':
        c = (order(a) - order(b)) * sign;
        break;
      default:
        c = (a.id - b.id) * sign;
    }
    return c !== 0 ? c : b.id - a.id;
  });
}

export interface TorrentSummary {
  /** Torrents that use the network. */
  active: number;
  downloading: number;
  seeding: number;
  down: number;
  up: number;
}

export function summarizeTorrents(items: readonly Torrent[]): TorrentSummary {
  const s: TorrentSummary = { active: 0, downloading: 0, seeding: 0, down: 0, up: 0 };
  for (const t of items) {
    if (!isActive(t)) continue;
    s.active++;
    if (t.status === 'downloading' || t.status === 'connecting') s.downloading++;
    if (t.status === 'seeding') s.seeding++;
    s.down += t.down_speed;
    s.up += t.up_speed;
  }
  return s;
}

/** Apply one live event to the list. Returns the same array when nothing changed. */
export function applyTorrentEvent(items: Torrent[], ev: AppEvent): Torrent[] {
  switch (ev.type) {
    case 'torrent': {
      const i = items.findIndex((t) => t.id === ev.torrent.id);
      if (i === -1) return [ev.torrent, ...items];
      const next = items.slice();
      next[i] = ev.torrent;
      return next;
    }
    case 'torrent_removed':
      return items.some((t) => t.id === ev.id) ? items.filter((t) => t.id !== ev.id) : items;
    default:
      return items;
  }
}

// ---- speed history (the chart of the details panel) -----------------------------------------

export interface SpeedSample {
  /** Milliseconds since the epoch. */
  at: number;
  down: number;
  up: number;
}

/** How long the chart looks back. */
export const HISTORY_MS = 3 * 60 * 1000;

/** Add a sample, dropping what is older than the window (but the one just before it). A sample that says nothing new (the
 *  same speeds as the last one, less than a second later) is not kept. */
export function pushSample(history: readonly SpeedSample[], s: SpeedSample): SpeedSample[] {
  const last = history[history.length - 1];
  if (last && last.down === s.down && last.up === s.up && s.at - last.at < 1000) {
    return history.slice();
  }
  const from = s.at - HISTORY_MS;
  // The last sample before the window stays: it is the speed the chart starts at.
  const firstInside = history.findIndex((x) => x.at >= from);
  const start = firstInside === -1 ? history.length - 1 : Math.max(0, firstInside - 1);
  const kept = history.slice(start);
  kept.push(s);
  return kept;
}

/** The samples as the points of a step chart over the window ending at `now`: a speed holds until
 *  the next sample, and the last one holds until `now`. */
export function chartPoints(
  history: readonly SpeedSample[],
  now: number,
): { x: number; down: number; up: number }[] {
  const from = now - HISTORY_MS;
  const out: { x: number; down: number; up: number }[] = [];
  let before: SpeedSample | undefined;
  for (const s of history) {
    if (s.at < from) {
      before = s;
      continue;
    }
    if (out.length === 0 && before) out.push({ x: 0, down: before.down, up: before.up });
    out.push({ x: (s.at - from) / HISTORY_MS, down: s.down, up: s.up });
  }
  if (out.length === 0 && before) out.push({ x: 0, down: before.down, up: before.up });
  const last = out[out.length - 1];
  if (last && last.x < 1) out.push({ x: 1, down: last.down, up: last.up });
  return out;
}

/** A round upper bound for the chart's vertical axis: 1, 2 or 5 times a power of ten. */
export function niceMax(value: number): number {
  if (!(value > 0)) return 1024;
  const exp = Math.pow(10, Math.floor(Math.log10(value)));
  const f = value / exp;
  const nice = f <= 1 ? 1 : f <= 2 ? 2 : f <= 5 ? 5 : 10;
  return nice * exp;
}
