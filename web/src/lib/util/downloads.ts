// Pure list logic: grouping, filtering, sorting, live updates.
import type { AppEvent, Category, Download, Status } from '../api/types';
import { fraction } from './format';

// ---- status groups (the tabs above the list) -------------------------------------

export type Group = 'all' | 'active' | 'paused' | 'completed' | 'stopped' | 'archived';

/** Tabs in display order. Their labels are translated: `group.<id>`. */
export const GROUPS: readonly { id: Group }[] = [
  { id: 'all' },
  { id: 'active' },
  { id: 'paused' },
  { id: 'completed' },
  { id: 'stopped' },
  { id: 'archived' },
];

export function groupOf(status: Status): Exclude<Group, 'all'> {
  switch (status) {
    case 'downloading':
    case 'queued':
      return 'active';
    case 'paused':
      return 'paused';
    case 'completed':
      return 'completed';
    default:
      return 'stopped'; // failed, cancelled
  }
}

/** The tab a download belongs to: archived ones have their own, whatever their status. */
export function groupOfDownload(d: Download): Exclude<Group, 'all'> {
  return d.archived ? 'archived' : groupOf(d.status);
}

// ---- filtering ---------------------------------------------------------------------

export interface Filter {
  group: Group;
  category: Category | 'all';
  query: string;
}

function matchesCategoryAndQuery(d: Download, f: Filter): boolean {
  if (f.category !== 'all' && d.category !== f.category) return false;
  const q = f.query.trim().toLowerCase();
  return !q || d.filename.toLowerCase().includes(q) || d.url.toLowerCase().includes(q);
}

export function matches(d: Download, f: Filter): boolean {
  // "All" is everything that is not archived: archiving hides a download from the main list.
  if (f.group === 'all' ? d.archived === true : groupOfDownload(d) !== f.group) return false;
  return matchesCategoryAndQuery(d, f);
}

export function filterDownloads(items: readonly Download[], f: Filter): Download[] {
  return items.filter((d) => matches(d, f));
}

/** Counts per group/category, each ignoring its own dimension so the chips stay useful. */
export function countGroups(
  items: readonly Download[],
  f: Filter,
): Record<Group, number> {
  const out: Record<Group, number> = {
    all: 0,
    active: 0,
    paused: 0,
    completed: 0,
    stopped: 0,
    archived: 0,
  };
  // Category and search apply; the group dimension is what is being counted.
  for (const d of items) {
    if (!matchesCategoryAndQuery(d, f)) continue;
    out[groupOfDownload(d)]++;
    if (!d.archived) out.all++;
  }
  return out;
}

export function countCategories(
  items: readonly Download[],
  f: Filter,
): Record<Category | 'all', number> {
  const base = items.filter((d) => matches(d, { ...f, category: 'all' }));
  const out: Record<Category | 'all', number> = {
    all: base.length,
    photo: 0,
    music: 0,
    video: 0,
    program: 0,
    archive: 0,
    other: 0,
  };
  for (const d of base) out[d.category]++;
  return out;
}

/** Current speed in bytes/s; the schema marks it optional (only running downloads have one). */
export function speedOf(d: Download): number {
  return d.speed ?? 0;
}

// ---- sorting -----------------------------------------------------------------------

export type SortKey = 'added' | 'name' | 'size' | 'progress' | 'speed' | 'status';
export type SortDir = 'asc' | 'desc';
export interface SortSpec {
  key: SortKey;
  dir: SortDir;
}

/** Sort choices in display order. Their labels are translated: `sort.<key>`. */
export const SORT_OPTIONS: readonly { key: SortKey }[] = [
  { key: 'added' },
  { key: 'name' },
  { key: 'size' },
  { key: 'progress' },
  { key: 'speed' },
  { key: 'status' },
];

/** Most interesting first: what is running, then waiting, then the rest. */
const STATUS_RANK: Record<Status, number> = {
  downloading: 0,
  queued: 1,
  paused: 2,
  failed: 3,
  cancelled: 4,
  completed: 5,
};

const collators = new Map<string, Intl.Collator>();

/** Natural, case-insensitive ordering by the language's own alphabet (Polish: a < ą < b). */
function collatorFor(locale: string | undefined): Intl.Collator {
  const key = locale ?? '';
  let c = collators.get(key);
  if (!c) {
    c = new Intl.Collator(locale, { numeric: true, sensitivity: 'base' });
    collators.set(key, c);
  }
  return c;
}

/** Compare numbers where `null` ("unknown") always sorts last, in both directions. */
function byNumber(a: number | null, b: number | null, dir: SortDir): number {
  if (a === null && b === null) return 0;
  if (a === null) return 1;
  if (b === null) return -1;
  return dir === 'asc' ? a - b : b - a;
}

/** New array; stable, ties broken by id (newest first) so the order never jitters. */
export function sortDownloads(
  items: readonly Download[],
  spec: SortSpec,
  locale?: string,
): Download[] {
  const sign = spec.dir === 'asc' ? 1 : -1;
  const collator = collatorFor(locale);
  const compare = (a: Download, b: Download): number => {
    switch (spec.key) {
      case 'added':
        return sign * (a.id - b.id);
      case 'name':
        return sign * collator.compare(a.filename, b.filename);
      case 'size':
        return byNumber(a.size ?? null, b.size ?? null, spec.dir);
      case 'progress':
        return byNumber(fraction(a.downloaded, a.size), fraction(b.downloaded, b.size), spec.dir);
      case 'speed':
        return sign * (speedOf(a) - speedOf(b));
      case 'status':
        return sign * (STATUS_RANK[a.status] - STATUS_RANK[b.status]);
    }
  };
  return [...items].sort((a, b) => compare(a, b) || b.id - a.id);
}

/** A sensible direction when the user switches to a new key. */
export function defaultDir(key: SortKey): SortDir {
  return key === 'name' || key === 'status' ? 'asc' : 'desc';
}

// ---- live updates ------------------------------------------------------------------

/** Apply one SSE event to the list. Returns the same array when nothing changed. */
export function applyEvent(items: Download[], ev: AppEvent): Download[] {
  switch (ev.type) {
    case 'download': {
      const i = items.findIndex((d) => d.id === ev.download.id);
      if (i === -1) return [ev.download, ...items];
      const next = items.slice();
      next[i] = ev.download;
      return next;
    }
    case 'progress': {
      const i = items.findIndex((d) => d.id === ev.id);
      const current = items[i];
      if (current === undefined) return items; // not loaded yet; the next list fetch has it
      const next = items.slice();
      next[i] = {
        ...current,
        downloaded: ev.downloaded,
        speed: ev.speed,
        size: ev.total ?? current.size,
      };
      return next;
    }
    case 'removed':
      return items.some((d) => d.id === ev.id) ? items.filter((d) => d.id !== ev.id) : items;
    default:
      return items; // `settings` is handled by the settings store
  }
}

export function summarize(items: readonly Download[]): { active: number; queued: number; speed: number } {
  let active = 0;
  let queued = 0;
  let speed = 0;
  for (const d of items) {
    if (d.status === 'downloading') {
      active++;
      speed += speedOf(d);
    } else if (d.status === 'queued') {
      queued++;
    }
  }
  return { active, queued, speed };
}
