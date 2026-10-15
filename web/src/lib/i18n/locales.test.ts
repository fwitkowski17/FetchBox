// Guards every language, including ones added later: nothing to maintain per language.
import { describe, expect, it } from 'vitest';
import { en } from './en';
import { LOCALES, FALLBACK_LOCALE } from './locales';
import { isPlural, placeholders, requiredPluralForms, translate } from './translate';
import type { Message } from './types';

const keys = Object.keys(en) as (keyof typeof en)[];
const entries = (m: Record<string, Message>) => Object.entries(m);

describe('locale registry', () => {
  it('has unique codes, native names and the fallback language', () => {
    const codes = LOCALES.map((l) => l.code);
    expect(new Set(codes).size).toBe(codes.length);
    expect(codes).toContain(FALLBACK_LOCALE);
    for (const l of LOCALES) expect(l.name.trim()).not.toBe('');
  });

  it('uses codes that Intl understands', () => {
    for (const l of LOCALES) {
      expect(Intl.NumberFormat.supportedLocalesOf(l.code)).toHaveLength(1);
      expect(Intl.PluralRules.supportedLocalesOf(l.code)).toHaveLength(1);
    }
  });
});

describe.each(LOCALES.map((l) => [l.code, l] as const))('dictionary "%s"', (_code, locale) => {
  it('has exactly the keys of the English source', async () => {
    const messages = await locale.load();
    expect(Object.keys(messages).sort()).toEqual([...keys].sort());
  });

  it('has no empty texts', async () => {
    const messages = await locale.load();
    for (const [key, m] of entries(messages)) {
      const forms = isPlural(m) ? Object.values(m) : [m];
      for (const f of forms) expect(f.trim(), key).not.toBe('');
    }
  });

  it('uses the same placeholders as English', async () => {
    const messages = await locale.load();
    for (const key of keys) {
      const expected = [...placeholders(en[key])].sort();
      const actual = [...placeholders(messages[key])].sort();
      expect(actual, key).toEqual(expected);
    }
  });

  it('has every plural form the language needs, and only where a count is passed', async () => {
    const messages = await locale.load();
    const needed = requiredPluralForms(locale.code);
    for (const key of keys) {
      const m = messages[key];
      if (!isPlural(m)) continue;
      for (const form of needed) expect(m[form], `${key}.${form}`).toBeTruthy();
      // The caller only passes `n` where English has an `{n}` placeholder.
      expect(placeholders(en[key]).has('n'), `${key} cannot be plural`).toBe(true);
    }
  });

  it('formats every plural message for a range of counts without leftovers', async () => {
    const messages = await locale.load();
    for (const key of keys) {
      const m = messages[key];
      if (!isPlural(m)) continue;
      for (const n of [0, 1, 2, 3, 4, 5, 11, 12, 21, 22, 25, 100, 101, 102]) {
        const text = translate(m, { n }, locale.code);
        expect(text, `${key} n=${n}`).toContain(String(n));
        expect(text, `${key} n=${n}`).not.toMatch(/\{\w+\}/);
      }
    }
  });
});

describe('English source', () => {
  it('only uses plurals where it has a count', () => {
    for (const [key, m] of entries(en)) {
      if (isPlural(m)) expect(placeholders(m).has('n'), key).toBe(true);
    }
  });
});
