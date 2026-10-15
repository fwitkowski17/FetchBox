import { describe, expect, it } from 'vitest';
import css from '../../app.css?raw';
import { en } from '../i18n/en';
import { ACCENTS, DEFAULT_ACCENT, accentById, isAccentId, swatch } from './accent';

// ---- colour maths: OKLCH -> sRGB with CSS-style gamut mapping, WCAG contrast ------------

type Rgb = [number, number, number];

function oklabToLinear(L: number, a: number, b: number): Rgb {
  const l = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3;
  const m = (L - 0.1055613458 * a - 0.0638541728 * b) ** 3;
  const s = (L - 0.0894841775 * a - 1.291485548 * b) ** 3;
  return [
    4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
    -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
    -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
  ];
}

const inGamut = (c: Rgb) => c.every((v) => v >= -1e-4 && v <= 1 + 1e-4);

/** Linear sRGB for an OKLCH colour; out-of-gamut chroma is reduced, as browsers do. */
function oklch(L: number, C: number, hDeg: number): Rgb {
  const h = (hDeg * Math.PI) / 180;
  const at = (c: number) => oklabToLinear(L, c * Math.cos(h), c * Math.sin(h));
  if (inGamut(at(C))) return at(C);
  let lo = 0;
  let hi = C;
  for (let i = 0; i < 30; i++) {
    const mid = (lo + hi) / 2;
    if (inGamut(at(mid))) lo = mid;
    else hi = mid;
  }
  return at(lo).map((v) => Math.min(1, Math.max(0, v))) as Rgb;
}

function hexToLinear(hex: string): Rgb {
  const n = parseInt(hex.slice(1), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255].map((v) => {
    const c = v / 255;
    return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  }) as Rgb;
}

function linearToOklab([r, g, b]: Rgb): [number, number, number] {
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
  return [
    0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s,
    1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s,
    0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s,
  ];
}

const luminance = ([r, g, b]: Rgb) => 0.2126 * r + 0.7152 * g + 0.0722 * b;
const contrast = (a: Rgb, b: Rgb) => {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x) as [number, number];
  return (hi + 0.05) / (lo + 0.05);
};

/** `color-mix(in oklab, accent pct, surface)`. */
function mix(accent: Rgb, surface: Rgb, pct: number): Rgb {
  const [a, b] = [linearToOklab(accent), linearToOklab(surface)];
  const t = pct / 100;
  const [L, A, B] = [0, 1, 2].map((i) => (a[i] as number) * t + (b[i] as number) * (1 - t)) as [number, number, number];
  return oklabToLinear(L, A, B).map((v) => Math.min(1, Math.max(0, v))) as Rgb;
}

// ---- read the theme tokens from app.css (so this test follows what really ships) ---------

function block(selector: string, from = 0): string {
  const start = css.indexOf(`${selector} {`, from);
  if (start < 0) throw new Error(`no block for ${selector}`);
  return css.slice(start, css.indexOf('}', start));
}

function token(b: string, name: string): string {
  const m = b.match(new RegExp(`${name}:\\s*([^;]+);`));
  if (!m?.[1]) throw new Error(`missing ${name}`);
  return m[1].trim();
}

const mediaStart = css.indexOf('@media (prefers-color-scheme: dark)');
const THEMES = {
  light: block(':root'),
  dark: block(":root[data-theme='dark']"),
  black: block(":root[data-theme='black']"),
  systemDark: block(":root:not([data-theme='light'])", mediaStart),
};
const KEYS = ['--accent-l', '--accent-c', '--accent-contrast', '--soft-mix', '--surface'];

describe('accent table', () => {
  it('has unique ids, sane values and a default', () => {
    expect(new Set(ACCENTS.map((a) => a.id)).size).toBe(ACCENTS.length);
    for (const a of ACCENTS) {
      expect(a.hue).toBeGreaterThanOrEqual(0);
      expect(a.hue).toBeLessThanOrEqual(360);
      expect(a.chroma).toBeGreaterThanOrEqual(0);
      expect(a.chroma).toBeLessThanOrEqual(1);
    }
    expect(isAccentId(DEFAULT_ACCENT)).toBe(true);
    expect(accentById(DEFAULT_ACCENT).id).toBe(DEFAULT_ACCENT);
  });

  it('validates ids', () => {
    expect(isAccentId('violet')).toBe(true);
    expect(isAccentId('chartreuse')).toBe(false);
    expect(isAccentId(7)).toBe(false);
  });

  it('is named in the dictionary, so a new accent cannot ship untranslated', () => {
    for (const a of ACCENTS) expect(`accent.${a.id}` in en, a.id).toBe(true);
  });

  it('draws swatches as oklch colours', () => {
    expect(swatch(accentById('blue'))).toBe('oklch(0.62 0.170 258)');
    expect(swatch(accentById('graphite'))).toBe('oklch(0.62 0.000 260)');
  });
});

describe('theme tokens', () => {
  it('keeps the system-dark block identical to the dark theme (the CSS repeats it)', () => {
    for (const k of KEYS) expect(token(THEMES.systemDark, k), k).toBe(token(THEMES.dark, k));
  });
});

describe.each(['light', 'dark', 'black'] as const)('accent contrast in the %s theme', (theme) => {
  const b = THEMES[theme];
  const L = Number(token(b, '--accent-l'));
  const C = Number(token(b, '--accent-c'));
  const onAccent = hexToLinear(token(b, '--accent-contrast'));
  const surface = hexToLinear(token(b, '--surface'));
  const softMix = Number(token(b, '--soft-mix').replace('%', ''));

  it.each(ACCENTS.map((a) => [a.id, a] as const))('%s is readable', (_id, a) => {
    const accent = oklch(L, C * a.chroma, a.hue);
    const soft = mix(accent, surface, softMix);
    // Primary buttons: label on the accent (hover brightens it slightly, hence the margin).
    expect(contrast(onAccent, accent), 'text on accent').toBeGreaterThanOrEqual(4.6);
    // Links, active menu items, percentages: accent as text on the card and on its soft tint.
    expect(contrast(accent, surface), 'accent on surface').toBeGreaterThanOrEqual(4.5);
    expect(contrast(accent, soft), 'accent on soft tint').toBeGreaterThanOrEqual(4.2);
  });
});
