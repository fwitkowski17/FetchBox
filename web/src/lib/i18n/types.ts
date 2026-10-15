// Shapes shared by the dictionaries and the translator. No runtime code here.

/** Plural forms keyed by CLDR category. `other` is mandatory; which of the others a language
 *  needs follows from `Intl.PluralRules` (English: one/other, Polish: one/few/many/other). */
export type Plural = { other: string } & Partial<Record<Intl.LDMLPluralRule, string>>;

/** A translated message: plain text, or plural forms chosen by the `n` parameter.
 *  `{name}` placeholders are replaced by the matching parameter. */
export type Message = string | Plural;

export type Params = Record<string, string | number>;
