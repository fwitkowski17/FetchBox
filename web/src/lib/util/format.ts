// Human-readable numbers. Pure functions, no DOM.

const UNITS = ['B', 'KB', 'MB', 'GB', 'TB'] as const;

const numberFormats = new Map<string, Intl.NumberFormat>();

/** Cached formatter with a fixed number of decimals and no digit grouping ("1536", not "1 536"). */
function decimals(locale: string, digits: number): Intl.NumberFormat {
  const key = `${locale}/${digits}`;
  let nf = numberFormats.get(key);
  if (!nf) {
    nf = new Intl.NumberFormat(locale, {
      minimumFractionDigits: digits,
      maximumFractionDigits: digits,
      useGrouping: false,
    });
    numberFormats.set(key, nf);
  }
  return nf;
}

/** `1536` -> `1.5 KB` (`1,5 KB` in Polish). Binary steps (1024), one decimal below 10. */
export function formatBytes(bytes: number, locale = 'en'): string {
  if (!Number.isFinite(bytes) || bytes < 0) return '—';
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < UNITS.length - 1) {
    value /= 1024;
    unit++;
  }
  let digits = unit === 0 || value >= 10 ? 0 : 1;
  // Rounding can land exactly on 1024 (e.g. 1023.96 KB): step up instead of showing it.
  if (unit > 0 && unit < UNITS.length - 1 && Number(value.toFixed(digits)) >= 1024) {
    value /= 1024;
    unit++;
    digits = 1;
  }
  return `${decimals(locale, digits).format(value)} ${UNITS[unit]}`;
}

/** How speeds are shown in the app: `auto` picks B, KB, MB... itself, `bits` picks bit, Kbit, Mbit...
 *  itself, anything else is one fixed unit. */
export type SpeedDisplay = 'auto' | 'bits' | SpeedUnit;

export const SPEED_DISPLAYS: readonly SpeedDisplay[] = ['auto', 'bits', 'KB', 'MB', 'Kbit', 'Mbit'];

const BIT_UNITS = ['bit', 'Kbit', 'Mbit', 'Gbit', 'Tbit'] as const;

/** Three significant figures at most: 123, 12.3, 1.23. */
function speedDigits(value: number): number {
  return value >= 100 ? 0 : value >= 10 ? 1 : 2;
}

export function formatSpeed(bytesPerSecond: number, locale = 'en', display: SpeedDisplay = 'auto'): string {
  if (display === 'auto') return `${formatBytes(bytesPerSecond, locale)}/s`;
  if (!Number.isFinite(bytesPerSecond) || bytesPerSecond < 0) return '—';
  if (display === 'bits') {
    let value = bytesPerSecond * 8;
    let unit = 0;
    while (value >= 1000 && unit < BIT_UNITS.length - 1) {
      value /= 1000;
      unit++;
    }
    let digits = unit === 0 ? 0 : speedDigits(value);
    // Rounding can land on 1000 (999.96 Kbit): step up instead of showing it.
    if (unit > 0 && unit < BIT_UNITS.length - 1 && Number(value.toFixed(digits)) >= 1000) {
      value /= 1000;
      unit++;
      digits = 2;
    }
    return `${decimals(locale, digits).format(value)} ${BIT_UNITS[unit]}/s`;
  }
  const value = bytesPerSecond / SPEED_FACTOR[display];
  return `${decimals(locale, speedDigits(value)).format(value)} ${SPEED_UNIT_LABEL[display]}`;
}

/** A duration split into the pieces of one unit pattern; the words around them are translated
 *  elsewhere (`duration.<unit>`). Numbers come as strings, zero-padded where that reads better. */
export interface DurationParts {
  unit: 's' | 'ms' | 'hm' | 'dh';
  values: Record<string, string>;
}

/** `null` for nonsense or anything of 100 days and more. */
export function durationParts(totalSeconds: number): DurationParts | null {
  if (!Number.isFinite(totalSeconds) || totalSeconds < 0) return null;
  const s = Math.round(totalSeconds);
  const pad = (n: number) => String(n).padStart(2, '0');
  if (s < 60) return { unit: 's', values: { s: String(s) } };
  if (s < 3600) return { unit: 'ms', values: { m: String(Math.floor(s / 60)), s: pad(s % 60) } };
  if (s < 86_400) {
    return { unit: 'hm', values: { h: String(Math.floor(s / 3600)), m: pad(Math.floor((s % 3600) / 60)) } };
  }
  const days = Math.floor(s / 86_400);
  if (days >= 100) return null;
  return { unit: 'dh', values: { d: String(days), h: String(Math.floor((s % 86_400) / 3600)) } };
}

/** Seconds left, or `null` when it cannot be known (no size or no speed). */
export function etaSeconds(
  downloaded: number,
  total: number | null | undefined,
  speed: number,
): number | null {
  if (total == null || total <= 0 || speed <= 0 || downloaded >= total) return null;
  return (total - downloaded) / speed;
}

/** 0..1, or `null` when the total size is unknown. */
export function fraction(downloaded: number, total: number | null | undefined): number | null {
  if (total == null || total <= 0) return null;
  return Math.min(1, Math.max(0, downloaded / total));
}

export function formatPercent(f: number | null): string {
  return f === null ? '' : `${Math.floor(f * 100)}%`;
}

/** Unix seconds -> date and time in the given language. */
export function formatDate(unixSeconds: number, locale = 'en'): string {
  return new Date(unixSeconds * 1000).toLocaleString(locale, {
    dateStyle: 'medium',
    timeStyle: 'short',
  });
}

/** Host name of a URL for compact display; the raw string when it does not parse. */
export function hostOf(url: string): string {
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
}

/** Quick client-side check; the server validates for real. */
export function isHttpUrl(text: string): boolean {
  try {
    const u = new URL(text.trim());
    return u.protocol === 'http:' || u.protocol === 'https:';
  } catch {
    return false;
  }
}

// ---- speed limits in the settings form -------------------------------------------

/** Bytes (binary: 1 KB = 1024 B) and bits (decimal, as network speeds are quoted: 1 Mbit = 10^6 bit). */
export type SpeedUnit = 'KB' | 'MB' | 'Kbit' | 'Mbit';

/** In the order the unit pickers show them. */
export const SPEED_UNITS: readonly SpeedUnit[] = ['KB', 'MB', 'Kbit', 'Mbit'];

export const SPEED_UNIT_LABEL: Record<SpeedUnit, string> = {
  KB: 'KB/s',
  MB: 'MB/s',
  Kbit: 'Kbit/s',
  Mbit: 'Mbit/s',
};

/** Bytes per second in one unit. */
const SPEED_FACTOR: Record<SpeedUnit, number> = {
  KB: 1024,
  MB: 1024 * 1024,
  Kbit: 1000 / 8,
  Mbit: 1_000_000 / 8,
};

/** Units tried in this order: the first one that shows the speed as a whole number wins, so what
 *  the user typed (100 Mbit/s) comes back as typed, not as 11.92 MB/s. */
const SPLIT_ORDER: readonly SpeedUnit[] = ['MB', 'KB', 'Mbit', 'Kbit'];

/** Bytes/s -> a number and the unit that shows it best; `0` means unlimited. */
export function splitSpeed(bytesPerSecond: number): { value: number; unit: SpeedUnit } {
  for (const unit of SPLIT_ORDER) {
    const factor = SPEED_FACTOR[unit];
    if (bytesPerSecond >= factor && bytesPerSecond % factor === 0) {
      return { value: bytesPerSecond / factor, unit };
    }
  }
  return { value: Math.round((bytesPerSecond / SPEED_FACTOR.KB) * 100) / 100, unit: 'KB' };
}

export function joinSpeed(value: number, unit: SpeedUnit): number {
  if (!Number.isFinite(value) || value <= 0) return 0;
  return Math.round(value * SPEED_FACTOR[unit]);
}
