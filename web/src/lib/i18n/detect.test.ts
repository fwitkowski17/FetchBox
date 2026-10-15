import { describe, expect, it } from 'vitest';
import { AUTO, resolveLocale } from './detect';

const available = ['en', 'pl', 'pt-BR'];
const pick = (pref: string, preferred: string[]) => resolveLocale(pref, preferred, available, 'en');

describe('resolveLocale', () => {
  it('honours an explicit choice over the browser', () => {
    expect(pick('pl', ['en-US'])).toBe('pl');
    expect(pick('PL', ['en-US'])).toBe('pl');
  });

  it('ignores an explicit choice we do not have', () => {
    expect(pick('de', ['pl'])).toBe('en');
  });

  it('follows the browser: exact, then same language, in order of preference', () => {
    expect(pick(AUTO, ['pl'])).toBe('pl');
    expect(pick(AUTO, ['pl-PL'])).toBe('pl');
    expect(pick(AUTO, ['de-DE', 'pl-PL', 'en'])).toBe('pl');
    expect(pick(AUTO, ['en-GB'])).toBe('en');
  });

  it('maps a regional variant to the one we have', () => {
    expect(pick(AUTO, ['pt-PT'])).toBe('pt-BR');
    expect(pick(AUTO, ['pt'])).toBe('pt-BR');
  });

  it('falls back when nothing matches or the browser says nothing', () => {
    expect(pick(AUTO, ['de', 'fr'])).toBe('en');
    expect(pick(AUTO, [])).toBe('en');
  });
});
