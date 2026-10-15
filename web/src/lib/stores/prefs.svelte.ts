// How the download list is sorted and filtered. Remembered between visits (not the search text).
import type { Category } from '../api/types';
import { CATEGORIES } from '../api/types';
import { GROUPS, SORT_OPTIONS, defaultDir, type Group, type SortKey, type SortSpec } from '../util/downloads';
import { readJson, writeJson } from '../util/storage';

const KEY = 'dlm.list';

interface Stored {
  sort: SortSpec;
  group: Group;
  category: Category | 'all';
}

const DEFAULTS: Stored = { sort: { key: 'added', dir: 'desc' }, group: 'all', category: 'all' };

function isStored(v: unknown): v is Stored {
  if (typeof v !== 'object' || v === null) return false;
  const o = v as Record<string, unknown>;
  const sort = o.sort as Record<string, unknown> | null | undefined;
  return (
    typeof sort === 'object' &&
    sort !== null &&
    SORT_OPTIONS.some((s) => s.key === sort.key) &&
    (sort.dir === 'asc' || sort.dir === 'desc') &&
    GROUPS.some((g) => g.id === o.group) &&
    (o.category === 'all' || CATEGORIES.includes(o.category as Category))
  );
}

class ListPrefs {
  #stored = $state<Stored>(readJson(KEY, DEFAULTS, isStored));
  query = $state('');

  get sort(): SortSpec {
    return this.#stored.sort;
  }
  get group(): Group {
    return this.#stored.group;
  }
  get category(): Category | 'all' {
    return this.#stored.category;
  }

  setSortKey(key: SortKey): void {
    this.#update({ sort: { key, dir: defaultDir(key) } });
  }
  toggleSortDir(): void {
    this.#update({ sort: { ...this.sort, dir: this.sort.dir === 'asc' ? 'desc' : 'asc' } });
  }
  setGroup(group: Group): void {
    this.#update({ group });
  }
  setCategory(category: Category | 'all'): void {
    this.#update({ category });
  }

  #update(patch: Partial<Stored>): void {
    this.#stored = { ...this.#stored, ...patch };
    writeJson(KEY, this.#stored);
  }
}

export const prefs = new ListPrefs();
