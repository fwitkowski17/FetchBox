// How the download list is sorted, filtered and drawn. Remembered between visits (not the search text).
import type { Category } from '../api/types';
import { defaultDir, type Group, type SortKey, type SortSpec, type ViewMode } from '../util/downloads';
import { readStored, type Stored } from '../util/listPrefs';
import { readJson, writeJson } from '../util/storage';

const KEY = 'fetchbox.list';

class ListPrefs {
  #stored = $state<Stored>(readStored(readJson<unknown>(KEY, null, (v): v is unknown => true)));
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
  get view(): ViewMode {
    return this.#stored.view;
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
  setView(view: ViewMode): void {
    this.#update({ view });
  }

  #update(patch: Partial<Stored>): void {
    this.#stored = { ...this.#stored, ...patch };
    writeJson(KEY, this.#stored);
  }
}

export const prefs = new ListPrefs();
