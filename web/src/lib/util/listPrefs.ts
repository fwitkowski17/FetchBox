// What the list remembers between visits, and how an older save is read.
import type { Category } from '../api/types';
import { CATEGORIES } from '../api/types';
import { GROUPS, SORT_OPTIONS, VIEW_MODES, type Group, type SortSpec, type ViewMode } from './downloads';

export interface Stored {
  sort: SortSpec;
  group: Group;
  category: Category | 'all';
  view: ViewMode;
}

export const DEFAULTS: Stored = {
  sort: { key: 'added', dir: 'desc' },
  group: 'all',
  category: 'all',
  view: 'cards',
};

export function isStored(v: unknown): v is Stored {
  if (typeof v !== 'object' || v === null) return false;
  const o = v as Record<string, unknown>;
  const sort = o.sort as Record<string, unknown> | null | undefined;
  return (
    typeof sort === 'object' &&
    sort !== null &&
    SORT_OPTIONS.some((s) => s.key === sort.key) &&
    (sort.dir === 'asc' || sort.dir === 'desc') &&
    GROUPS.some((g) => g.id === o.group) &&
    (o.category === 'all' || CATEGORIES.includes(o.category as Category)) &&
    VIEW_MODES.includes(o.view as ViewMode)
  );
}

/** What was saved, made usable: a save from before the views existed keeps its sort and filters
 *  and shows cards, as it always did; anything else that does not fit gives the defaults. */
export function readStored(raw: unknown): Stored {
  if (isStored(raw)) return raw;
  if (typeof raw === 'object' && raw !== null && !('view' in raw)) {
    const upgraded = { ...raw, view: DEFAULTS.view };
    if (isStored(upgraded)) return upgraded;
  }
  return DEFAULTS;
}
