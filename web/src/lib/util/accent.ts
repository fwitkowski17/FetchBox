// The accent colours the user can choose from. Only the hue differs: the theme decides lightness
// and saturation (app.css), which keeps text on the accent readable in light, dark and black.

export interface Accent {
  id: string;
  /** OKLCH hue angle, 0-360. */
  hue: number;
  /** Multiplies the theme's saturation; some hues look garish at full strength, graphite has none. */
  chroma: number;
}

/** Names are translated: `accent.<id>`. */
export const ACCENTS = [
  { id: 'blue', hue: 258, chroma: 1 },
  { id: 'violet', hue: 292, chroma: 1 },
  { id: 'pink', hue: 350, chroma: 1 },
  { id: 'red', hue: 25, chroma: 1 },
  { id: 'orange', hue: 52, chroma: 1 },
  { id: 'green', hue: 150, chroma: 0.8 },
  { id: 'teal', hue: 195, chroma: 0.75 },
  { id: 'graphite', hue: 260, chroma: 0 },
] as const satisfies readonly Accent[];

export type AccentId = (typeof ACCENTS)[number]['id'];

export const DEFAULT_ACCENT: AccentId = 'blue';

export const isAccentId = (v: unknown): v is AccentId => ACCENTS.some((a) => a.id === v);

export function accentById(id: AccentId): Accent {
  return ACCENTS.find((a) => a.id === id) ?? ACCENTS[0];
}

/** A colour that shows the accent in the picker, independent of the current theme. */
export function swatch(a: Accent): string {
  return `oklch(0.62 ${(0.17 * a.chroma).toFixed(3)} ${a.hue})`;
}
