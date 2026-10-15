// The list of languages. To add one:
//   1. copy `en.ts` to `xx.ts` (xx = language code) and translate the values;
//   2. add one line below.
// The compiler rejects a missing or misspelled key, `locales.test.ts` checks placeholders and
// plural forms, and the language shows up in Settings by itself.
import { en, type Messages } from './en';

export interface LocaleInfo {
  /** BCP 47 code, e.g. `pl` or `pt-BR`. Also used for number, date and plural rules. */
  code: string;
  /** The language's own name, shown in the picker (never translated). */
  name: string;
  /** Loads the dictionary. Non-default languages are separate chunks, fetched on demand. */
  load: () => Promise<Messages>;
}

export const FALLBACK_LOCALE = 'en';

export const LOCALES: readonly LocaleInfo[] = [
  { code: 'en', name: 'English', load: async () => en },
];
