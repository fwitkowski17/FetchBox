// What the disk ring shows. Pure functions, no DOM.
import type { DiskSpace } from '../api/types';

/** From this share of the disk in use the ring turns amber / red. */
export const WARN_AT = 0.85;
export const FULL_AT = 0.95;

export type DiskLevel = 'ok' | 'warn' | 'full';

export interface DiskView {
  /** Share of the disk in use, 0..1. */
  used: number;
  /** Share that unfinished downloads will still fill (capped at what is free), 0..1. */
  reserved: number;
  level: DiskLevel;
  /** Bytes the unfinished downloads need beyond the free space; 0 when everything fits. */
  short: number;
}

const clamp01 = (n: number): number => Math.min(1, Math.max(0, n));

export function diskView(d: DiskSpace): DiskView {
  const usedBytes = Math.max(0, d.total - d.available);
  const used = d.total > 0 ? clamp01(usedBytes / d.total) : 0;
  const reserved = d.total > 0 ? clamp01(Math.min(d.reserved, d.available) / d.total) : 0;
  const short = Math.max(0, d.reserved - d.available);
  let level: DiskLevel = 'ok';
  if (short > 0 || used >= FULL_AT) level = 'full';
  else if (used >= WARN_AT) level = 'warn';
  return { used, reserved, level, short };
}
