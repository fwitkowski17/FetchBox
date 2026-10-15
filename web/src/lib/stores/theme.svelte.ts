// Light / dark / follow-the-system. The choice is stored; the actual colours live in CSS
// (`[data-theme]` overrides, otherwise `prefers-color-scheme`).
import { readJson, writeJson } from '../util/storage';

export type ThemePref = 'system' | 'light' | 'dark' | 'black';

const KEY = 'fetchbox.theme';
const isPref = (v: unknown): v is ThemePref =>
  v === 'system' || v === 'light' || v === 'dark' || v === 'black';

/** Order of the quick toggle. */
const ORDER: readonly ThemePref[] = ['system', 'light', 'dark', 'black'];

class ThemeStore {
  pref = $state<ThemePref>(readJson<ThemePref>(KEY, 'system', isPref));
  #systemDark = $state(false);

  /** What is actually shown right now (`system` resolves to light or dark, never black). */
  effective = $derived<'light' | 'dark' | 'black'>(
    this.pref === 'system' ? (this.#systemDark ? 'dark' : 'light') : this.pref,
  );

  /** Apply the stored choice and follow OS changes. Call once at startup. */
  init(): void {
    const mq = window.matchMedia('(prefers-color-scheme: dark)');
    this.#systemDark = mq.matches;
    mq.addEventListener('change', (e) => {
      this.#systemDark = e.matches;
      this.#apply();
    });
    this.#apply();
  }

  set(pref: ThemePref): void {
    this.pref = pref;
    writeJson(KEY, pref);
    this.#apply();
  }

  /** Quick toggle: system -> light -> dark -> black -> system. */
  cycle(): void {
    this.set(ORDER[(ORDER.indexOf(this.pref) + 1) % ORDER.length] ?? 'system');
  }

  #apply(): void {
    const root = document.documentElement;
    if (this.pref === 'system') {
      root.removeAttribute('data-theme');
    } else {
      root.setAttribute('data-theme', this.pref);
    }
    // Colour of the browser chrome (mobile address bar, installed-app title bar).
    const bg = getComputedStyle(root).getPropertyValue('--bg').trim();
    if (bg) {
      document.querySelector('meta[name="theme-color"]')?.setAttribute('content', bg);
    }
  }
}

export const theme = new ThemeStore();
