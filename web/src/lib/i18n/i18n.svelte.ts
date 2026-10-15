// The active language. Reactive: anything that calls `t(...)` in a template or `$derived`
// re-renders when the language changes.
import { ApiError } from '../api/client';
import {
  SPEED_DISPLAYS,
  durationParts,
  formatBytes,
  formatDate,
  formatSpeed,
  type SpeedDisplay,
} from '../util/format';
import { readJson, writeJson } from '../util/storage';
import { AUTO, resolveLocale } from './detect';
import { en, type MessageKey, type Messages } from './en';
import { FALLBACK_LOCALE, LOCALES } from './locales';
import { translate, translateParts, type Part } from './translate';
import type { Params } from './types';

const KEY = 'fetchbox.locale';
const SPEED_KEY = 'fetchbox.speedUnit';
const CODES = LOCALES.map((l) => l.code);
const isSpeedDisplay = (v: unknown): v is SpeedDisplay => SPEED_DISPLAYS.includes(v as SpeedDisplay);
const isPref = (v: unknown): v is string => v === AUTO || (typeof v === 'string' && CODES.includes(v));

class I18n {
  /** What the user chose: `auto` (follow the browser) or a language code. */
  pref = $state<string>(readJson<string>(KEY, AUTO, isPref));
  /** The language actually in use. */
  locale = $state<string>(FALLBACK_LOCALE);
  /** How speeds are written everywhere in the app (a display choice, not part of the settings). */
  speedUnit = $state<SpeedDisplay>(readJson<SpeedDisplay>(SPEED_KEY, 'auto', isSpeedDisplay));
  #messages = $state.raw<Messages>(en);

  /** Load the dictionary for the stored choice. Await it before the first render. */
  init(): Promise<void> {
    return this.#activate(this.pref);
  }

  /** Switch language (or back to `auto`) and remember it. */
  async setPref(pref: string): Promise<void> {
    const next = isPref(pref) ? pref : AUTO;
    await this.#activate(next);
    this.pref = next;
    writeJson(KEY, next);
  }

  setSpeedUnit(unit: SpeedDisplay): void {
    this.speedUnit = isSpeedDisplay(unit) ? unit : 'auto';
    writeJson(SPEED_KEY, this.speedUnit);
  }

  async #activate(pref: string): Promise<void> {
    const wanted = resolveLocale(pref, navigator.languages ?? [], CODES, FALLBACK_LOCALE);
    let code = wanted;
    let messages: Messages = en;
    try {
      const info = LOCALES.find((l) => l.code === wanted);
      if (info) messages = await info.load();
    } catch {
      // A chunk that fails to load (offline, stale cache) must not break the app.
      code = FALLBACK_LOCALE;
    }
    this.#messages = messages;
    this.locale = code;
    document.documentElement.lang = code;
  }

  // Arrow properties so they can be passed around (`const { t } = i18n`) and stay reactive.

  /** Translate `key`. Plural messages take their form from `params.n`. */
  t = (key: MessageKey, params?: Params): string =>
    translate(this.#messages[key] ?? en[key], params, this.locale);

  /** Like `t`, but returns the pieces so substituted values can be styled (see `Trans`). */
  parts = (key: MessageKey, params?: Params): Part[] =>
    translateParts(this.#messages[key] ?? en[key], params, this.locale);

  bytes = (n: number): string => formatBytes(n, this.locale);
  speed = (n: number): string => formatSpeed(n, this.locale, this.speedUnit);
  date = (unixSeconds: number): string => formatDate(unixSeconds, this.locale);

  duration = (seconds: number): string => {
    const p = durationParts(seconds);
    return p ? this.t(`duration.${p.unit}`, p.values) : '—';
  };

  /** A message for the user, from anything that was thrown. */
  errorText = (e: unknown): string => {
    if (e instanceof ApiError) {
      if (e.status === 0) return this.t('error.network');
      if (e.status === 507 && e.space) {
        return this.t('error.noSpace', {
          needed: this.bytes(e.space.needed),
          free: this.bytes(Math.max(0, e.space.free - e.space.reserved)),
        });
      }
      // The server's own wording is shown as it is (the backend is not translated).
      return e.serverMessage ?? this.t('error.http', { status: e.status });
    }
    return this.t('error.unexpected');
  };
}

export const i18n = new I18n();
export const t = i18n.t;
