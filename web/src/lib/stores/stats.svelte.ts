// The statistics page's data: one summary for the chosen period. Reloaded while the page is open.
import { api } from '../api/client';
import type { Stats, StatsRange } from '../api/types';
import { readJson, writeJson } from '../util/storage';

const KEY = 'fetchbox.statsRange';
export const RANGES: readonly StatsRange[] = ['today', '7d', '30d', 'all'];
const isRange = (v: unknown): v is StatsRange => RANGES.includes(v as StatsRange);

class StatsStore {
  range = $state<StatsRange>(readJson<StatsRange>(KEY, 'all', isRange));
  /** `null` until the first answer. */
  value = $state.raw<Stats | null>(null);
  /** The error of the last failed load (shown through `i18n.errorText`). */
  error = $state.raw<unknown>(null);

  /** Answers can arrive in another order than they were asked for (a quick change of period). */
  #latest = 0;

  setRange(range: StatsRange): void {
    if (range === this.range) return;
    this.range = range;
    writeJson(KEY, range);
    void this.load();
  }

  async load(): Promise<void> {
    const mine = ++this.#latest;
    const range = this.range;
    try {
      // East of UTC is positive; `getTimezoneOffset` counts the other way.
      const result = await api.getStats(range, -new Date().getTimezoneOffset());
      if (mine !== this.#latest) return;
      this.value = result;
      this.error = null;
    } catch (e) {
      if (mine === this.#latest) this.error = e;
    }
  }

  clear(): void {
    this.#latest++;
    this.value = null;
    this.error = null;
  }
}

export const stats = new StatsStore();
