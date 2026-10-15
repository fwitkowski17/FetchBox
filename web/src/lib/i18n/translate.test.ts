import { describe, expect, it } from 'vitest';
import { placeholders, requiredPluralForms, translate, translateParts } from './translate';

describe('translate', () => {
  it('replaces placeholders and leaves unknown ones visible', () => {
    expect(translate('Hello {name}', { name: 'Ann' }, 'en')).toBe('Hello Ann');
    expect(translate('{a} and {b}', { a: 1 }, 'en')).toBe('1 and {b}');
    expect(translate('plain', undefined, 'en')).toBe('plain');
  });

  it('replaces every occurrence and accepts numbers', () => {
    expect(translate('{n}/{n}', { n: 3 }, 'en')).toBe('3/3');
  });

  it('does not treat user data as a template', () => {
    expect(translate('File {name}', { name: '{n}' }, 'en')).toBe('File {n}');
  });

  it('picks English plural forms', () => {
    const m = { one: '{n} file', other: '{n} files' };
    expect(translate(m, { n: 1 }, 'en')).toBe('1 file');
    expect(translate(m, { n: 0 }, 'en')).toBe('0 files');
    expect(translate(m, { n: 5 }, 'en')).toBe('5 files');
  });

  it('picks Polish plural forms (one / few / many)', () => {
    const m = { one: '{n} plik', few: '{n} pliki', many: '{n} plików', other: '{n} pliku' };
    const pick = (n: number) => translate(m, { n }, 'pl');
    expect(pick(1)).toBe('1 plik');
    expect(pick(2)).toBe('2 pliki');
    expect(pick(4)).toBe('4 pliki');
    expect(pick(5)).toBe('5 plików');
    expect(pick(12)).toBe('12 plików');
    expect(pick(22)).toBe('22 pliki');
    expect(pick(25)).toBe('25 plików');
    expect(pick(0)).toBe('0 plików');
  });

  it('falls back to "other" when a form is missing or n is not a number', () => {
    expect(translate({ other: 'x {n}' }, { n: 1 }, 'en')).toBe('x 1');
    expect(translate({ one: 'one', other: 'many' }, {}, 'en')).toBe('many');
    expect(translate({ one: 'one', other: 'many' }, { n: 'abc' }, 'en')).toBe('many');
  });
});

describe('translateParts', () => {
  it('separates literal text from substituted values', () => {
    expect(translateParts('A file named {name} exists ({size}).', { name: 'a.zip', size: '3 MB' }, 'en')).toEqual([
      { text: 'A file named ', value: false },
      { text: 'a.zip', value: true },
      { text: ' exists (', value: false },
      { text: '3 MB', value: true },
      { text: ').', value: false },
    ]);
  });

  it('handles messages that start or end with a value and unknown placeholders', () => {
    expect(translateParts('{a}{b}', { a: 'x' }, 'en')).toEqual([
      { text: 'x', value: true },
      { text: '{b}', value: false },
    ]);
    expect(translateParts('plain', undefined, 'en')).toEqual([{ text: 'plain', value: false }]);
  });

  it('keeps markup in user data as text', () => {
    const parts = translateParts('Name {n}', { n: '<b>x</b>' }, 'en');
    expect(parts[1]).toEqual({ text: '<b>x</b>', value: true });
  });
});

describe('helpers', () => {
  it('lists placeholders across plural forms', () => {
    expect([...placeholders('Hi {a} {b} {a}')].sort()).toEqual(['a', 'b']);
    expect([...placeholders({ one: '{n} x', other: '{n} y {z}' })].sort()).toEqual(['n', 'z']);
    expect(placeholders('none').size).toBe(0);
  });

  it('knows which plural forms a language needs', () => {
    expect(requiredPluralForms('en').sort()).toEqual(['one', 'other']);
    expect(requiredPluralForms('pl').sort()).toEqual(['few', 'many', 'one', 'other']);
  });
});
