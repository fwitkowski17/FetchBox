// The accent colour. Only the hue (and a saturation factor) is chosen here; lightness and the
// rest come from the active theme in app.css, which keeps text on the accent readable.
import { ACCENTS, DEFAULT_ACCENT, accentById, isAccentId, type AccentId } from '../util/accent';
import { readJson, writeJson } from '../util/storage';

const KEY = 'fetchbox.accent';

interface Stored {
  id: AccentId;
}

const isStored = (v: unknown): v is Stored =>
  typeof v === 'object' && v !== null && isAccentId((v as Record<string, unknown>).id);

class AccentStore {
  id = $state<AccentId>(readJson<Stored>(KEY, { id: DEFAULT_ACCENT }, isStored).id);

  /** Apply the stored choice. Call once at startup. */
  init(): void {
    this.#apply();
  }

  set(id: AccentId): void {
    this.id = id;
    this.#apply();
  }

  #apply(): void {
    const a = accentById(this.id);
    const root = document.documentElement.style;
    root.setProperty('--accent-h', String(a.hue));
    root.setProperty('--accent-k', String(a.chroma));
    // `index.html` reads the numbers back before the first paint, without needing this table.
    writeJson(KEY, { id: a.id, hue: a.hue, chroma: a.chroma });
  }
}

export const accent = new AccentStore();
export { ACCENTS };
