// The declarative interface of a plugin (SPEC 6.15): the pure rules for showing its fields,
// checking what was typed and reading the answer of an action. Plugin text is data: it is shown
// as text and never as markup, links go through `isSafeExternalUrl`, icons through `iconOrDefault`.
import type {
  ActionResult,
  NoticeLevel,
  PluginField,
  PluginFieldView,
  PluginView,
  SettingValue,
} from '../api/types';

/** The values of a form, by field key. `''` is an empty text/number/select. */
export type Values = Record<string, SettingValue>;

/** A plugin's own text in the language of the interface: `i18n[locale][text]`, then the bare
 *  language (`pl` for `pl-PL`), else the text itself (the English original). */
export function tr(i18n: PluginView['i18n'], locale: string, text: string): string {
  if (text === '') return text;
  const lang = locale.split(/[-_]/)[0] ?? locale;
  for (const l of [locale, lang]) {
    const hit = i18n[l]?.[text];
    if (hit !== undefined) return hit;
  }
  return text;
}

/** Fields that hold a value (the others only show something). */
export function isInput(field: PluginField): boolean {
  return field.type !== 'info' && field.type !== 'output';
}

function emptyFor(field: PluginField): SettingValue {
  return field.type === 'toggle' ? false : '';
}

/** The values a settings form starts with: what is stored, else the default; a secret starts
 *  empty (its value never reaches the browser). */
export function valuesFromViews(views: readonly PluginFieldView[]): Values {
  const out: Values = {};
  for (const v of views) {
    if (!isInput(v.field)) continue;
    if (v.field.type === 'secret') out[v.field.key] = '';
    else out[v.field.key] = v.value ?? v.field.default ?? emptyFor(v.field);
  }
  return out;
}

/** The values a dialog starts with: the defaults. */
export function valuesFromFields(fields: readonly PluginField[]): Values {
  const out: Values = {};
  for (const f of fields) if (isInput(f)) out[f.key] = f.default ?? emptyFor(f);
  return out;
}

/** A field is shown while the field it depends on has the wanted value. */
export function isShown(field: PluginField, values: Values): boolean {
  const cond = field.show_if;
  if (!cond) return true;
  return values[cond.key] === cond.equals;
}

export type FieldError =
  | { code: 'required' }
  | { code: 'number' }
  | { code: 'min'; limit: number }
  | { code: 'max'; limit: number };

/** Problems with the values typed, by field key. Hidden fields are not judged. `has` lists the
 *  secrets that already have a stored value (empty is then fine). */
export function validateValues(
  fields: readonly PluginField[],
  values: Values,
  has: ReadonlySet<string> = new Set(),
): Record<string, FieldError> {
  const errors: Record<string, FieldError> = {};
  for (const f of fields) {
    if (!isInput(f) || !isShown(f, values)) continue;
    const v = values[f.key];
    if (f.type === 'number') {
      if (v === '' || v === undefined) {
        if (f.required) errors[f.key] = { code: 'required' };
        continue;
      }
      const n = typeof v === 'number' ? v : Number(v);
      if (typeof v === 'boolean' || !Number.isFinite(n)) errors[f.key] = { code: 'number' };
      else if (f.min != null && n < f.min) errors[f.key] = { code: 'min', limit: f.min };
      else if (f.max != null && n > f.max) errors[f.key] = { code: 'max', limit: f.max };
    } else if (f.required && f.type !== 'toggle') {
      const empty = typeof v !== 'string' || v.trim() === '';
      if (empty && !(f.type === 'secret' && has.has(f.key))) errors[f.key] = { code: 'required' };
    }
  }
  return errors;
}

/** Numbers typed into a field arrive as text: turn them into numbers before sending. */
export function normalizeValues(fields: readonly PluginField[], values: Values): Values {
  const out: Values = {};
  for (const f of fields) {
    if (!isInput(f)) continue;
    const v = values[f.key];
    if (v === undefined) continue;
    out[f.key] = f.type === 'number' && typeof v === 'string' && v.trim() !== '' ? Number(v) : v;
  }
  return out;
}

export interface SettingsChange {
  values: Values;
  /** Keys whose stored value is to be removed. */
  clear: string[];
}

/** What to send to save the form: only fields that differ from what is stored. An emptied field
 *  that had a value is cleared (back to its default); a secret is only sent when something was
 *  typed, and cleared when `clearSecrets` names it. Hidden fields are left alone. */
export function settingsChange(
  views: readonly PluginFieldView[],
  draft: Values,
  clearSecrets: ReadonlySet<string> = new Set(),
): SettingsChange {
  const fields = views.map((v) => v.field);
  const draftNormalized = normalizeValues(fields, draft);
  const values: Values = {};
  const clear: string[] = [];
  for (const view of views) {
    const f = view.field;
    if (!isInput(f) || !isShown(f, draft)) continue;
    const v = draftNormalized[f.key];
    if (f.type === 'secret') {
      if (clearSecrets.has(f.key) && v === '') clear.push(f.key);
      else if (typeof v === 'string' && v !== '') values[f.key] = v;
      continue;
    }
    const stored = view.value ?? null;
    if (v === undefined || v === '') {
      if (stored !== null) clear.push(f.key);
    } else if (v !== (stored ?? f.default ?? null)) {
      values[f.key] = v;
    }
  }
  return { values, clear };
}

export function hasChange(c: SettingsChange): boolean {
  return Object.keys(c.values).length > 0 || c.clear.length > 0;
}

/** Only web addresses are opened; anything else a plugin names is ignored. */
export function isSafeExternalUrl(url: string | null | undefined): url is string {
  if (!url) return false;
  try {
    const u = new URL(url);
    return (u.protocol === 'https:' || u.protocol === 'http:') && u.host !== '';
  } catch {
    return false;
  }
}

/** The icon a plugin asked for if it is one of ours, else `fallback`. */
export function iconOrDefault<T extends string>(
  name: string | null | undefined,
  allowed: readonly T[],
  fallback: T,
): T {
  return allowed.find((n) => n === name) ?? fallback;
}

export interface ActionOutcome {
  ok: boolean;
  notice: { level: NoticeLevel; message: string } | null;
  close: boolean;
  openDialog: string | null;
  /** New values for fields of the open dialog (an `output` field shows its text). */
  set: Values;
}

/** Read the answer of an action. A failure without a message still says it failed. */
export function actionOutcome(r: ActionResult, failedText: string): ActionOutcome {
  const message = r.message?.trim() ? r.message : null;
  const level: NoticeLevel = r.level ?? (r.ok ? 'success' : 'error');
  return {
    ok: r.ok,
    notice: message ? { level, message } : r.ok ? null : { level: 'error', message: failedText },
    close: r.close === true,
    openDialog: r.open_dialog ?? null,
    set: { ...(r.set ?? {}) },
  };
}
