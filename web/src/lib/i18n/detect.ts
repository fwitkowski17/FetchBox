// Which language to use: the user's explicit choice, else the browser's, else the fallback.

/** Stored choice meaning "follow the browser". */
export const AUTO = 'auto';

const base = (code: string): string => code.toLowerCase().split('-')[0] ?? '';

/**
 * @param pref       `AUTO` or a language code the user picked
 * @param preferred  browser languages, best first (`navigator.languages`)
 * @param available  codes we have translations for
 */
export function resolveLocale(
  pref: string,
  preferred: readonly string[],
  available: readonly string[],
  fallback: string,
): string {
  const exact = (code: string) => available.find((a) => a.toLowerCase() === code.toLowerCase());
  if (pref !== AUTO) return exact(pref) ?? fallback;

  for (const p of preferred) {
    const hit = exact(p);
    if (hit) return hit;
    // "pl-PL" -> "pl", "pt-PT" -> any "pt-*" we have
    const sameLanguage = available.filter((a) => base(a) === base(p));
    const hitByLanguage = sameLanguage.find((a) => a.toLowerCase() === base(p)) ?? sameLanguage[0];
    if (hitByLanguage) return hitByLanguage;
  }
  return fallback;
}
