// Human-readable numbers. Pure functions, no DOM.

const UNITS = ['B', 'KB', 'MB', 'GB', 'TB'] as const;

/** `1536` -> `1.5 KB`. Binary steps (1024), one decimal below 10. */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return '—';
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < UNITS.length - 1) {
    value /= 1024;
    unit++;
  }
  let text = unit === 0 ? String(Math.round(value)) : value.toFixed(value < 10 ? 1 : 0);
  // Rounding can land exactly on 1024 (e.g. 1023.96 KB): step up instead of showing it.
  if (unit > 0 && unit < UNITS.length - 1 && Number(text) >= 1024) {
    value /= 1024;
    unit++;
    text = value.toFixed(1);
  }
  return `${text} ${UNITS[unit]}`;
}

export function formatSpeed(bytesPerSecond: number): string {
  return `${formatBytes(bytesPerSecond)}/s`;
}

/** Seconds -> `45 s`, `12 min 05 s`, `2 h 03 min`, `1 d 4 h`. */
export function formatDuration(totalSeconds: number): string {
  if (!Number.isFinite(totalSeconds) || totalSeconds < 0) return '—';
  const s = Math.round(totalSeconds);
  if (s < 60) return `${s} s`;
  const pad = (n: number) => String(n).padStart(2, '0');
  if (s < 3600) return `${Math.floor(s / 60)} min ${pad(s % 60)} s`;
  if (s < 86_400) return `${Math.floor(s / 3600)} h ${pad(Math.floor((s % 3600) / 60))} min`;
  const days = Math.floor(s / 86_400);
  if (days >= 100) return '—';
  return `${days} d ${Math.floor((s % 86_400) / 3600)} h`;
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

/** Unix seconds -> local date and time. */
export function formatDate(unixSeconds: number): string {
  return new Date(unixSeconds * 1000).toLocaleString(undefined, {
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

// ---- speed limits in the settings form -------------------------------------------

export type SpeedUnit = 'KB' | 'MB';

const SPEED_FACTOR: Record<SpeedUnit, number> = { KB: 1024, MB: 1024 * 1024 };

/** Bytes/s -> a number and the unit that shows it best; `0` means unlimited. */
export function splitSpeed(bytesPerSecond: number): { value: number; unit: SpeedUnit } {
  if (bytesPerSecond >= SPEED_FACTOR.MB && bytesPerSecond % SPEED_FACTOR.MB === 0) {
    return { value: bytesPerSecond / SPEED_FACTOR.MB, unit: 'MB' };
  }
  return { value: Math.round((bytesPerSecond / SPEED_FACTOR.KB) * 100) / 100, unit: 'KB' };
}

export function joinSpeed(value: number, unit: SpeedUnit): number {
  if (!Number.isFinite(value) || value <= 0) return 0;
  return Math.round(value * SPEED_FACTOR[unit]);
}
