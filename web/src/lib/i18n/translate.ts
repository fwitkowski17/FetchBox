// Pure message formatting: placeholders and plurals. No state, no DOM.
import type { Message, Params, Plural } from './types';

const rules = new Map<string, Intl.PluralRules>();

function pluralRules(locale: string): Intl.PluralRules {
  let r = rules.get(locale);
  if (!r) {
    r = new Intl.PluralRules(locale);
    rules.set(locale, r);
  }
  return r;
}

/** The text to use for `params.n` (plain messages are returned as they are). */
function choose(message: Message, params: Params | undefined, locale: string): string {
  if (typeof message === 'string') return message;
  const n = Number(params?.n);
  if (!Number.isFinite(n)) return message.other;
  return message[pluralRules(locale).select(n)] ?? message.other;
}

const PLACEHOLDER = /\{(\w+)\}/g;

/** Unknown placeholders stay visible (`{x}`) so a missing parameter is noticed, not hidden. */
export function translate(message: Message, params: Params | undefined, locale: string): string {
  return choose(message, params, locale).replace(PLACEHOLDER, (whole, name: string) =>
    params && name in params ? String(params[name]) : whole,
  );
}

/** A message split into literal text and substituted values, so the UI can style the values
 *  (e.g. bold file names) without ever building HTML from user data. */
export interface Part {
  text: string;
  /** `true` for a substituted parameter. */
  value: boolean;
}

export function translateParts(message: Message, params: Params | undefined, locale: string): Part[] {
  const text = choose(message, params, locale);
  const parts: Part[] = [];
  let last = 0;
  for (const m of text.matchAll(PLACEHOLDER)) {
    const index = m.index ?? 0;
    if (index > last) parts.push({ text: text.slice(last, index), value: false });
    const name = m[1] ?? '';
    parts.push(
      params && name in params
        ? { text: String(params[name]), value: true }
        : { text: m[0], value: false },
    );
    last = index + m[0].length;
  }
  if (last < text.length) parts.push({ text: text.slice(last), value: false });
  return parts;
}

/** Placeholder names used by a message, across all of its plural forms. */
export function placeholders(message: Message): Set<string> {
  const forms = typeof message === 'string' ? [message] : Object.values(message);
  const names = new Set<string>();
  for (const form of forms) {
    for (const m of form.matchAll(PLACEHOLDER)) names.add(m[1] ?? '');
  }
  return names;
}

/** Plural categories a language needs, e.g. Polish: one, few, many, other. */
export function requiredPluralForms(locale: string): Intl.LDMLPluralRule[] {
  return pluralRules(locale).resolvedOptions().pluralCategories;
}

export function isPlural(message: Message): message is Plural {
  return typeof message !== 'string';
}
